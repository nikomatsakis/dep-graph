// Copyright 2014 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::hash::Hash;
use std::rc::Rc;
use std::usize;

use super::safe::DepGraphSafe;

#[derive(Clone)]
pub struct DepGraph<N>
    where N: DepNodeName,
{
    data: Rc<DepGraphData<N>>
}

pub trait DepNodeName: Clone + Debug + Eq + Hash {
}

#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub struct DepNodeIndex {
    index: usize
}

pub type Map<K, V> = HashMap<K, V>;

pub type Set<K> = HashSet<K>;

struct DepGraphData<N>
    where N: DepNodeName,
{
    enabled: bool,

    nodes: RefCell<DepGraphNodes<N>>,
}

struct DepGraphNodes<N>
    where N: DepNodeName,
{
    node_data: Vec<DepNodeData<N>>,

    /// Hash-set used to canonicalize these vectors.
    predecessors: Set<Rc<Vec<DepNodeIndex>>>,

    task_stack: Vec<TaskStackEntry>,

    /// Anonymous nodes are named by their predecessors.  This map
    /// goes from a (canonically sorted) set of predecessors to an
    /// anonymous node index, so we can re-use indices that occur
    /// regularly.
    anon_node_map: Map<Rc<Vec<DepNodeIndex>>, DepNodeIndex>,

    /// Quickly look up the named node.
    named_node_map: Map<N, DepNodeIndex>,
}

struct DepNodeData<N> {
    opt_name: Option<N>,
    predecessors: Rc<Vec<DepNodeIndex>>,
}

/// For each active task, we push one of these entries, which
/// accumulates a list of dep-nodes that were accessed. The set is
/// used to quickly check if a given pred has been accessed already;
/// the vec stores the list of preds in the order in which they were
/// accessed (it's important to preserve that ordering to prevent us
/// from doing extra work and so forth).
struct TaskStackEntry {
    predecessors: Vec<DepNodeIndex>,
    predecessor_set: Set<DepNodeIndex>,
}

impl<N> DepGraph<N>
    where N: DepNodeName,
{
    pub fn new(enabled: bool) -> Self {
        DepGraph {
            data: Rc::new(DepGraphData {
                enabled,
                nodes: RefCell::new(DepGraphNodes::new()),
            })
        }
    }

    /// True if we are actually building the full dep-graph.
    #[inline]
    pub fn is_fully_enabled(&self) -> bool {
        self.data.enabled
    }

    /// Executes `op`, ignoring any dependencies. Used to "hack" the
    /// system -- use with care! Once red-green system is in place,
    /// probably not much needed anymore.
    pub fn with_ignore<OP,R>(&self, op: OP) -> R
        where OP: FnOnce() -> R
    {
        if !self.data.enabled {
            op()
        } else {
            self.data.nodes.borrow_mut().push_task();
            let result = op();
            let _ = self.data.nodes.borrow_mut().pop_task(None);
            result
        }
    }

    /// Starts a new dep-graph task. Dep-graph tasks are specified
    /// using a free function (`task`) and **not** a closure -- this
    /// is intentional because we want to exercise tight control over
    /// what state they have access to. In particular, we want to
    /// prevent implicit 'leaks' of tracked state into the task (which
    /// could then be read without generating correct edges in the
    /// dep-graph -- see the [README] for more details on the
    /// dep-graph). To this end, the task function gets exactly two
    /// pieces of state: the context `cx` and an argument `arg`. Both
    /// of these bits of state must be of some type that implements
    /// `DepGraphSafe` and hence does not leak.
    ///
    /// The choice of two arguments is not fundamental. One argument
    /// would work just as well, since multiple values can be
    /// collected using tuples. However, using two arguments works out
    /// to be quite convenient, since it is common to need a context
    /// (`cx`) and some argument (e.g., a `DefId` identifying what
    /// item to process).
    ///
    /// For cases where you need some other number of arguments:
    ///
    /// - If you only need one argument, just use `()` for the `arg`
    ///   parameter.
    /// - If you need 3+ arguments, use a tuple for the
    ///   `arg` parameter.
    ///
    /// [README]: README.md
    pub fn with_task<C, A, R>(&self, key: N, cx: C, arg: A, task: fn(C, A) -> R) -> R
        where C: DepGraphSafe, A: DepGraphSafe
    {
        self.with_task_internal(Some(key), cx, arg, task).0
    }

    /// Like `with_task`, but it creates an **anonymous task**. The
    /// only way to name this task later is through its
    /// `DepNodeIndex`.
    pub fn with_anon_task<C, A, R>(&self,
                                   cx: C,
                                   arg: A,
                                   task: fn(C, A) -> R)
                                   -> (R, DepNodeIndex)
        where C: DepGraphSafe, A: DepGraphSafe
    {
        self.with_task_internal(None, cx, arg, task)
    }

    fn with_task_internal<C, A, R>(&self,
                                   key: Option<N>,
                                   cx: C,
                                   arg: A,
                                   task: fn(C, A) -> R)
                                   -> (R, DepNodeIndex)
        where C: DepGraphSafe, A: DepGraphSafe
    {
        if !self.data.enabled {
            return (task(cx, arg), DepNodeIndex::dummy())
        } else {
            self.data.nodes.borrow_mut().push_task();
            let result = task(cx, arg);
            let node_index = self.data.nodes.borrow_mut().pop_task(key);
            (result, node_index)
        }
    }

    /// Indicates that the current task read the data at `v`.
    pub fn read(&self, v: DepNodeIndex) {
        if self.data.enabled {
            self.data.nodes.borrow_mut().read(v);
        } else {
            debug_assert!(v.is_dummy());
        }
    }

    /// Lower-level interface to starting/stopping a task.  This
    /// interface does not guarantee isolation of the task (i.e., it
    /// can access whatever it wants), so we don't expose this
    /// broadly, but it's convenient for building up layers.
    pub(super) fn push_task(&self) {
        if self.data.enabled {
            self.data.nodes.borrow_mut().push_task();
        }
    }

    /// Lower-level interface for stopping a task.
    pub(super) fn pop_task(&self, name: Option<N>) -> DepNodeIndex {
        if self.data.enabled {
            self.data.nodes.borrow_mut().pop_task(name)
        } else {
            DepNodeIndex::dummy()
        }
    }
}

impl<N> DepGraphNodes<N>
    where N: DepNodeName,
{
    fn new() -> Self {
        DepGraphNodes {
            node_data: Default::default(),
            predecessors: Default::default(),
            task_stack: Default::default(),
            anon_node_map: Default::default(),
            named_node_map: Default::default(),
        }
    }

    fn push_task(&mut self) {
        self.task_stack.push(TaskStackEntry {
            predecessors: vec![],
            predecessor_set: Set::new(),
        });
    }

    /// Finishes the current task and creates a node to represent it.
    /// The node will be anonymous if `opt_name` is `None`, and named
    /// otherwise.
    fn pop_task(&mut self, opt_name: Option<N>) -> DepNodeIndex {
        let entry = self.task_stack.pop().unwrap();

        // Canonicalize the vector of predecessors.
        let predecessors = if let Some(s) = self.predecessors.get(&entry.predecessors).cloned() {
            s
        } else {
            let vec = Rc::new(entry.predecessors);
            self.predecessors.insert(vec.clone());
            vec
        };

        // Check if a suitable anonymous node already exists.
        //
        // Micro-optimization: this could be improved by taking
        // advantage of the fact that vectors of predecessors are
        // interned above.
        if opt_name.is_none() {
            if let Some(&index) = self.anon_node_map.get(&predecessors) {
                return index;
            }
        }

        // Otherwise, we have to make a new node. If this is a named node,
        // it should not already exist.
        let index = DepNodeIndex { index: self.node_data.len() };
        if let Some(ref n) = opt_name {
            let prev = self.named_node_map.insert(n.clone(), index);
            assert!(prev.is_none(), "created named node {:?} twice", n);
        }
        self.node_data.push(DepNodeData { opt_name, predecessors });

        index
    }

    fn read(&mut self, v: DepNodeIndex) {
        if let Some(top) = self.task_stack.last_mut() {
            if top.predecessor_set.insert(v) {
                top.predecessors.push(v);
            }
        }
    }
}

impl DepNodeIndex {
    fn dummy() -> Self {
        DepNodeIndex { index: usize::MAX }
    }

    fn is_dummy(self) -> bool {
        self.index == usize::MAX
    }
}

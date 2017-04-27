// Copyright 2012-2015 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use std::cell::{Cell, RefCell, RefMut, Ref};
use std::collections::HashSet;

use super::{DepGraph, DepNodeName, DepNodeIndex};
use super::safe::DepGraphSafe;

/// A DepTrackingMap offers a subset of the `Map` API and ensures that
/// we make calls to `read` and `write` as appropriate. We key the
/// maps with a unique type for brevity.
pub struct DepCell<T> {
    state: Cell<State>,
    value: RefCell<T>,
}

#[derive(Copy, Clone, Debug)]
enum State {
    Unlocked(DepNodeIndex),
    Locked(TaskId),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct TaskId(usize);

impl<N> DepGraph<N>
    where N: DepNodeName
{
    pub fn cell_task<'cx, C, A, R>(
        &self,
        cx: C,
        arg: A,
        user_task_fn: for<'task> fn(C, A, &mut Task<'task, 'cx, N>) -> R)
        -> R
        where C: DepGraphSafe + 'cx,
              A: DepGraphSafe + 'cx,
    {
        let task_id = TaskId(0); // TODO fix this
        let mut task = Task {
            graph: self.clone(),
            task_id: &task_id,
            locked: vec![],
            read_locked_set: HashSet::new(),
        };
        self.push_task();
        let result = user_task_fn(cx, arg, &mut task);
        let node = self.pop_task(None);
        task.release_locks(node);
        result
    }

    pub fn new_cell<C, A, R>(&self,
                             cx: C,
                             arg: A,
                             user_task_fn: fn(C, A) -> R)
        -> DepCell<R>
        where C: DepGraphSafe,
              A: DepGraphSafe,
    {
        let (value, node) = self.with_anon_task(cx, arg, user_task_fn);
        DepCell {
            state: Cell::new(State::Unlocked(node)),
            value: RefCell::new(value)
        }
    }
}

pub struct Task<'task, 'cx, N>
    where N: DepNodeName, 'cx: 'task,
{
    graph: DepGraph<N>,
    task_id: &'task TaskId,
    read_locked_set: HashSet<*const Cell<State>>,
    locked: Vec<&'cx Cell<State>>,
}

impl<'task, 'cx, N> Task<'task, 'cx, N>
    where N: DepNodeName, 'cx: 'task,
{
    pub fn borrow_mut<T: 'cx>(&mut self, cell: &'cx DepCell<T>) -> RefMut<'cx, T> {
        match cell.state.get() {
            State::Unlocked(node) => {
                self.graph.read(node);
                cell.state.set(State::Locked(*self.task_id));
                self.locked.push(&cell.state);
            }

            State::Locked(task_id) => {
                if task_id != *self.task_id {
                    panic!("cannot write -- cell locked by another task ({:?})", task_id)
                }
            }
        }

        cell.value.borrow_mut()
    }

    pub fn borrow<T: 'cx>(&mut self, cell: &'cx DepCell<T>) -> Ref<'cx, T> {
        match cell.state.get() {
            State::Unlocked(node) => {
                self.graph.read(node);
            }

            State::Locked(task_id) => {
                if task_id != *self.task_id {
                    panic!("cannot read -- cell locked by another task ({:?})", task_id)
                }
            }
        }

        cell.value.borrow()
    }

    fn release_locks(self, node: DepNodeIndex) {
        for locked_state in self.locked {
            match locked_state.replace(State::Unlocked(node)) {
                State::Unlocked(_) => unreachable!(),
                State::Locked(task_id) => debug_assert_eq!(task_id, *self.task_id),
            }
        }
    }
}

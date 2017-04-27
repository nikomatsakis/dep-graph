// Copyright 2012-2015 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use std::cell::{Cell, UnsafeCell};
use std::rc::Rc;

use super::{DepGraph, DepNodeName, DepNodeIndex};
use super::safe::DepGraphSafe;

/// A DepTrackingMap offers a subset of the `Map` API and ensures that
/// we make calls to `read` and `write` as appropriate. We key the
/// maps with a unique type for brevity.
pub struct DepCell<T> {
    data: Rc<DepCellData<T>>
}

struct DepCellData<T> {
    state: Cell<State>,
    value: UnsafeCell<T>,
}

// This trait is used to allow us to interact with a `DepCellData`
// without knowing the precise value `T`.
trait DepCellDataTrait {
    fn state(&self) -> &Cell<State>;
}

#[derive(Copy, Clone, Debug)]
enum State {
    Unlocked(DepNodeIndex),
    ReadLocked(TaskId),
    WriteLocked(TaskId),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct TaskId(usize);

impl<T> Clone for DepCell<T> {
    fn clone(&self) -> Self {
        DepCell { data: self.data.clone() }
    }
}

impl<N> DepGraph<N>
    where N: DepNodeName
{
    pub fn cell_task<'data, C, A, R>(
        &self,
        cx: C,
        arg: A,
        user_task_fn: for<'task> fn(C, A, &mut Task<'task, 'data, N>) -> R)
        -> R
        where C: DepGraphSafe, A: DepGraphSafe
    {
        let task_id = TaskId(0); // TODO fix this
        let mut task = Task { graph: self.clone(), task_id: &task_id, locked: vec![] };
        self.push_task();
        let result = user_task_fn(cx, arg, &mut task);
        let node = self.pop_task(None);
        task.release_locks(node);
        result
    }
}

pub struct Task<'task, 'data, N>
    where N: DepNodeName, 'data: 'task,
{
    graph: DepGraph<N>,
    task_id: &'task TaskId,
    locked: Vec<Rc<DepCellDataTrait + 'data>>,
}

impl<'task, 'data, N> Task<'task, 'data, N>
    where N: DepNodeName, 'data: 'task,
{
    pub fn cell<T: 'data>(&mut self, value: T) -> (DepCell<T>, &'task mut T) {
        let cell = DepCell {
            data: Rc::new(DepCellData {
                state: Cell::new(State::WriteLocked(*self.task_id)),
                value: UnsafeCell::new(value)
            })
        };
        self.locked.push(cell.data.clone());
        (cell.clone(), unsafe { &mut *cell.data.value.get() })
    }

    pub fn borrow_mut<T: 'data>(&mut self, cell: &DepCell<T>) -> &'task mut T {
        match cell.data.state.get() {
            State::Unlocked(_) => { }
            State::ReadLocked(_) | State::WriteLocked(_) => {
                panic!("cannot write -- cell already in state {:?}", cell.data.state.get());
            }
        }

        match cell.data.state.replace(State::WriteLocked(*self.task_id)) {
            State::Unlocked(u) => self.graph.read(u),
            State::ReadLocked(_) | State::WriteLocked(_) => { }
        }

        self.locked.push(cell.data.clone());
        unsafe { &mut *cell.data.value.get() }
    }

    pub fn borrow<T: 'data>(&mut self, cell: &DepCell<T>) -> &'task T {
        match cell.data.state.get() {
            State::Unlocked(_) => { }
            State::ReadLocked(task) => {
                if task != *self.task_id {
                    panic!("cannot read -- cell already in state {:?}", cell.data.state.get());
                }
            }
            State::WriteLocked(_) => {
                panic!("cannot read -- cell already in state {:?}", cell.data.state.get());
            }
        }

        match cell.data.state.replace(State::ReadLocked(*self.task_id)) {
            State::Unlocked(u) => self.graph.read(u),
            State::ReadLocked(_) | State::WriteLocked(_) => { }
        }

        self.locked.push(cell.data.clone());
        unsafe { &*cell.data.value.get() }
    }

    fn release_locks(self, node: DepNodeIndex) {
        for locked in self.locked {
            let locked_state = locked.state();
            match locked_state.get() {
                State::Unlocked(_) => unreachable!(),
                State::ReadLocked(task_id) => {
                    assert_eq!(task_id, *self.task_id);
                }
                State::WriteLocked(task_id) => {
                    assert_eq!(task_id, *self.task_id);
                    locked_state.set(State::Unlocked(node));
                }
            }
        }
    }
}

impl<T> DepCellDataTrait for DepCellData<T> {
    fn state(&self) -> &Cell<State> {
        &self.state
    }
}

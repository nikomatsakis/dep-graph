#![cfg(test)]

use super::*;

struct TestCx<'tcx> {
    graph: DepGraph<()>,
    mirs: Vec<DepCell<Mir<'tcx>>>,
}

impl<'tcx> TestCx<'tcx> {
    pub fn new() -> Self {
        TestCx {
            graph: DepGraph::new(true),
            mirs: vec![],
        }
    }

    fn add_mir(&mut self) {
        let cell = self.graph.cell_task((), (), make_mir);

        fn make_mir<'tcx, 'task>((): (),
                                 (): (),
                                 task: &mut Task<'task, 'tcx, ()>)
                                 -> DepCell<Mir<'tcx>>
        {
            task.cell(Mir::new()).0
        }

        self.mirs.push(cell);
    }
}

impl DepNodeName for () { }
impl DepGraphSafe for usize { } 

impl<'tcx> DepGraphSafe for TestCx<'tcx> { }

struct Mir<'tcx> {
    data: &'tcx u32,
    counter: usize,
}

impl<'tcx> Mir<'tcx> {
    pub fn new() -> Self {
        static C: u32 = 22;
        Mir { data: &C, counter: 0 }
    }
}

#[test]
fn basic_usage() {
    let mut cx = TestCx::new();
    cx.add_mir();

    cx.graph.cell_task(&cx, 1, inc_counters);
    fn inc_counters<'task, 'a, 'tcx>(cx: &'a TestCx<'tcx>,
                                     amount: usize,
                                     task: &mut Task<'task, 'a, ()>) {
        for c in &cx.mirs {
            let m = task.borrow_mut(c);
            m.counter += amount;
        }
    }
}

#[test]
#[should_panic]
fn borrow_mut_twice() {
    let mut cx = TestCx::new();
    cx.add_mir();

    cx.graph.cell_task(&cx, 1, inc_counters);
    fn inc_counters<'task, 'a, 'tcx>(cx: &'a TestCx<'tcx>,
                                     _: usize,
                                     task: &mut Task<'task, 'a, ()>) {
        for c in &cx.mirs {
            task.borrow_mut(c);
            task.borrow_mut(c);
        }
    }
}

#[test]
fn borrow_twice() {
    let mut cx = TestCx::new();
    cx.add_mir();

    cx.graph.cell_task(&cx, 0, verify_counters);
    fn verify_counters<'task, 'a, 'tcx>(cx: &'a TestCx<'tcx>,
                                        amount: usize,
                                        task: &mut Task<'task, 'a, ()>) {
        for c in &cx.mirs {
            task.borrow(c);
            let m = task.borrow(c);
            assert_eq!(m.counter, amount);
        }
    }
}

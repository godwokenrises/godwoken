use gw_common::smt::SMT;

pub trait SMTTree<S> {
    fn smt_tree(&self) -> SMT<S>;
}

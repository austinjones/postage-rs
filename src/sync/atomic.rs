pub enum AtomicResult<A, P> {
    Apply(A),
    Pending(P),
}

pub enum VerifyResult<A> {
    Apply(A),
    Pending,
}

pub trait AtomicApply {
    type Apply;
    type Pending;

    fn apply(&self) -> AtomicResult<Self::Action, Self::Pending>;

    fn verify(
        &self,
        pending: Self::Pending,
        context: &mut Context<'_>,
    ) -> VerifyResult<Self::Action>;
}

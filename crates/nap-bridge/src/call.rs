use std::{fmt, sync::Arc};

use nmp_native_runtime_core::{
    ApprovedWrite, BoundedJson, Cancellation, ReceiptEventSink, WorkLease,
};

#[derive(Debug)]
pub struct ProviderCall {
    pub response: Option<BoundedJson>,
    operation: Option<ProviderOperation>,
    write_proposal: Option<Box<ProviderWriteProposal>>,
}

impl ProviderCall {
    pub fn completed(response: Option<BoundedJson>) -> Self {
        Self {
            response,
            operation: None,
            write_proposal: None,
        }
    }

    /// Returns an active operation whose work permit remains charged until the
    /// caller explicitly completes or cancels it (or drops the response).
    pub fn streaming(response: Option<BoundedJson>, work: WorkLease) -> Self {
        Self {
            response,
            operation: Some(ProviderOperation::new(work)),
            write_proposal: None,
        }
    }

    /// Returns an exact write proposal for native review.
    ///
    /// Constructing a proposal does not accept a durable write. The caller
    /// must approve the exact [`ApprovedWrite`], convert the one-shot
    /// completion into a receipt sink, and pass both through the runtime's
    /// single `accept_write` call.
    pub fn proposed_write(
        response: Option<BoundedJson>,
        write: ApprovedWrite,
        completion: Box<dyn ProviderWriteCompletion>,
        work: WorkLease,
    ) -> Self {
        Self {
            response,
            operation: None,
            write_proposal: Some(Box::new(ProviderWriteProposal {
                write: Some(write),
                completion: Some(completion),
                work: Some(work),
            })),
        }
    }

    pub fn operation(&self) -> Option<&ProviderOperation> {
        self.operation.as_ref()
    }

    pub fn take_operation(&mut self) -> Option<ProviderOperation> {
        self.operation.take()
    }

    pub fn write_proposal(&self) -> Option<&ProviderWriteProposal> {
        self.write_proposal.as_deref()
    }

    pub fn take_write_proposal(&mut self) -> Option<ProviderWriteProposal> {
        self.write_proposal.take().map(|proposal| *proposal)
    }

    pub fn is_active(&self) -> bool {
        self.operation.is_some() || self.write_proposal.is_some()
    }
}

/// One exact provider-originated write awaiting native approval.
///
/// The proposal retains its admitted-work lease until it is approved,
/// refused, or dropped. Consuming it for approval transfers the exact write
/// and its one-shot NAP completion together, preventing either half from
/// being accidentally reused with another approval.
#[derive(Debug)]
pub struct ProviderWriteProposal {
    pub write: Option<ApprovedWrite>,
    completion: Option<Box<dyn ProviderWriteCompletion>>,
    work: Option<WorkLease>,
}

impl ProviderWriteProposal {
    pub fn into_parts(mut self) -> (ApprovedWrite, Box<dyn ProviderWriteCompletion>, WorkLease) {
        let write = self
            .write
            .take()
            .expect("a retained write proposal always owns its approved write");
        let completion = self
            .completion
            .take()
            .expect("a retained write proposal always owns its completion");
        let work = self
            .work
            .take()
            .expect("a retained write proposal always owns its work lease");
        (write, completion, work)
    }

    pub fn refuse(mut self, reason: Arc<str>) {
        if let Some(completion) = self.completion.take() {
            completion.refused(reason);
        }
        if let Some(work) = self.work.take() {
            work.cancellation().cancel();
        }
    }
}

impl Drop for ProviderWriteProposal {
    fn drop(&mut self) {
        if let Some(work) = self.work.take() {
            work.cancellation().cancel();
        }
    }
}

/// One-shot continuation for a provider write after native approval.
///
/// The completion becomes a receipt sink before the runtime calls
/// `HostDataPlane::accept_write`, allowing the app to fan out its own receipt
/// projection and the provider's protocol result through one observation.
pub trait ProviderWriteCompletion: Send + Sync + fmt::Debug {
    fn into_receipt_sink(self: Box<Self>) -> Arc<dyn ReceiptEventSink>;
    fn refused(self: Box<Self>, reason: Arc<str>);
}

/// The lifecycle owner for one active provider operation.
///
/// Providers clone the cancellation signal before returning the operation and
/// stop their native work when it is signalled. The work permit remains
/// charged while this value is retained. Dropping it is a cancellation path,
/// while [`ProviderOperation::complete`] records a normal terminal path.
#[derive(Debug)]
pub struct ProviderOperation {
    work: Option<WorkLease>,
}

impl ProviderOperation {
    fn new(work: WorkLease) -> Self {
        Self { work: Some(work) }
    }

    pub fn cancellation(&self) -> &Cancellation {
        self.work
            .as_ref()
            .expect("an owned provider operation always retains its work lease")
            .cancellation()
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancellation().is_cancelled()
    }

    pub fn complete(mut self) {
        self.work.take();
    }

    pub fn cancel(mut self) {
        if let Some(work) = self.work.take() {
            work.cancellation().cancel();
        }
    }
}

impl Drop for ProviderOperation {
    fn drop(&mut self) {
        if let Some(work) = self.work.take() {
            work.cancellation().cancel();
        }
    }
}

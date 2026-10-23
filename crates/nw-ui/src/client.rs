//! Presentation depends on this client port, never on the engine or store.
use nw_domain::commands::{Request, Response};
pub type Reply = Box<dyn FnOnce(Result<Response, String>) + Send>;
pub trait CommandClient: Send + Sync {
    fn choose_media(&self,reply:Reply);
    #[cfg(target_arch="wasm32")]
    fn upload_dropped(&self,file:web_sys::File,reply:Reply);
    fn send(&self, request: Request, reply: Reply);
}

//! Presentation depends on this client port, never on the engine or store.
use nw_domain::commands::{Request, Response};
pub type SaveReply = Box<dyn FnOnce(Result<nw_domain::download::SaveReceipt, String>) + Send>;
pub type Reply = Box<dyn FnOnce(Result<Response, String>) + Send>;
pub trait CommandClient: Send + Sync {
    fn choose_media(&self, reply: Reply);
    #[cfg(target_arch = "wasm32")]
    fn upload_dropped(&self, file: web_sys::File, reply: Reply);
    fn save_artifact(
        &self,
        manifest: nw_domain::artifacts::ArtifactManifest,
        destination: Option<String>,
        control: std::sync::Arc<nw_domain::download::SaveControl>,
        reply: SaveReply,
    );
    fn send(&self, request: Request, reply: Reply);
}

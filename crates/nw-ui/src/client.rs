//! Presentation depends on this client port, never on the engine or store.
use nw_domain::commands::{Request, Response};
pub type SaveReply = Box<dyn FnOnce(Result<nw_domain::download::SaveReceipt, String>) + Send>;
pub type Reply = Box<dyn FnOnce(Result<Response, String>) + Send>;
pub trait CommandClient: Send + Sync {
    fn choose_media(&self, reply: Reply);
    #[cfg(target_arch = "wasm32")]
    fn upload_dropped(&self, file: web_sys::File, reply: Reply);
    #[cfg(target_arch = "wasm32")]
    fn open_browser_playback(
        &self,
        segment: nw_domain::preview::SegmentPlayback,
        label: &'static str,
    );
    #[cfg(target_arch = "wasm32")]
    fn open_browser_program(&self, request: nw_domain::preview::ProgramPlaybackRequest);
    #[cfg(target_arch = "wasm32")]
    fn close_browser_playback(&self);
    #[cfg(not(target_arch = "wasm32"))]
    fn supports_native_picture(&self) -> bool {
        false
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn play_native_picture(
        &self,
        _request: nw_domain::preview::native_wire::OpenRequest,
        _mailbox: crate::NativePictureMailbox,
        _ctx: eframe::egui::Context,
    ) {
    }
    fn save_artifact(
        &self,
        manifest: nw_domain::artifacts::ArtifactManifest,
        destination: Option<String>,
        control: std::sync::Arc<nw_domain::download::SaveControl>,
        reply: SaveReply,
    );
    fn send(&self, request: Request, reply: Reply);
}

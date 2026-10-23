//! An immutable executable namespace cannot serve arbitrary project/media bytes.
use axum::{Router, response::{IntoResponse, Response}, extract::Path, routing::get, http::{StatusCode, header}};
pub struct Asset {pub path:&'static str,pub mime:&'static str,pub immutable:bool,pub bytes:&'static [u8]}
include!(concat!(env!("OUT_DIR"),"/embedded_web.rs"));
fn response(path:&str)->Response {
    match ASSETS.iter().find(|asset|asset.path==path) {
        Some(asset)=>([(header::CONTENT_TYPE,asset.mime),(header::CACHE_CONTROL,if asset.immutable{"public, max-age=31536000, immutable"}else{"no-store"})],asset.bytes).into_response(),
        None=>(StatusCode::NOT_FOUND,"NW_ASSET_MISSING: use the matching CI-packaged application").into_response(),
    }
}
pub fn router()->Router {
    Router::new().route("/",get(||async{response("index.html")})).route("/assets/{name}",get(|Path(name):Path<String>|async move{response(&format!("assets/{name}"))}))
}

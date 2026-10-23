//! Build the shared WASM client and a hash-addressed, offline executable asset namespace.
use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command};
fn run(command:&mut Command)->Result<()> {
    tracing::info!(argv=?command,"web.build.prepared");
    if !command.status()?.success(){bail!("Web build tool failed");} Ok(())
}
fn asset(root:&Path, bytes:&[u8], extension:&str, mime:&str)->Result<serde_json::Value>{
    let digest=hex::encode(Sha256::digest(bytes));
    let path=format!("assets/{digest}.{extension}");fs::write(root.join(&path),bytes)?;
    Ok(serde_json::json!({"path":path,"sha256":digest,"bytes":bytes.len(),"mime":mime,"immutable":true}))
}
pub fn build(root:&Path)->Result<()> {
    if std::env::var("GITHUB_ACTIONS").as_deref()!=Ok("true"){bail!("Web compilation belongs in GitHub CI");}
    let source=std::env::var("GITHUB_SHA").context("CI source identity")?;
    let tools:serde_json::Value=serde_json::from_slice(&fs::read(root.join("components/build-tools.json"))?)?;
    let tool=&tools["wasm_bindgen"];
    let version=tool["version"].as_str().context("wasm-bindgen version")?;
    let lock=fs::read_to_string(root.join("Cargo.lock"))?;
    if !lock.contains(&format!("name = \"wasm-bindgen\"\nversion = \"{version}\"")){bail!("Locked wasm-bindgen and the selected CLI disagree");}
    let directory=root.join("target/tools/wasm-bindgen");fs::create_dir_all(&directory)?;
    let archive=directory.join("tool.tar.gz");
    run(Command::new("curl").args(["--fail","--location","--proto","=https","--tlsv1.2","--max-time","120","--max-filesize","33554432","--output"]).arg(&archive).arg(tool["url"].as_str().context("tool URL")?))?;
    let bytes=fs::read(&archive)?;
    if bytes.len()>32*1024*1024 || tool["sha256"].as_str()!=Some(hex::encode(Sha256::digest(&bytes)).as_str()){bail!("Official wasm-bindgen tool digest mismatch");}
    run(Command::new("tar").arg("-xzf").arg(&archive).args(["--strip-components=1","--no-same-owner","--no-same-permissions","-C"]).arg(&directory))?;
    run(Command::new("cargo").current_dir(root).args(["build","--locked","-p","nw-ui","--target","wasm32-unknown-unknown","--release"]).env("NW_UI_BUILD_SHA",&source))?;
    let output=root.join("target/web-ui");let generated=root.join("target/web-generated");
    fs::create_dir_all(output.join("assets"))?;fs::create_dir_all(&generated)?;
    run(Command::new(directory.join("wasm-bindgen")).args(["--target","web","--no-typescript","--out-name","nw_ui","--out-dir"]).arg(&generated).arg(root.join("target/wasm32-unknown-unknown/release/nw_ui.wasm")))?;
    let wasm=asset(&output,&fs::read(generated.join("nw_ui_bg.wasm"))?,"wasm","application/wasm")?;
    let wasm_name=wasm["path"].as_str().context("wasm asset")?.strip_prefix("assets/").context("asset namespace")?;
    let glue=fs::read_to_string(generated.join("nw_ui.js"))?.replace("nw_ui_bg.wasm",wasm_name);
    let js=asset(&output,glue.as_bytes(),"js","application/javascript")?;
    let js_name=js["path"].as_str().context("glue asset")?.strip_prefix("assets/").context("asset namespace")?;
    let bootstrap=format!(r#"import init, {{ WebHandle }} from './{js_name}';
const status = document.getElementById('boot-status');
let app;
const proof = location.hash;
if (proof.startsWith('#pair=')) history.replaceState(null, '', location.pathname + location.search);
try {{
  await init();
  app = new WebHandle();
  await app.start(document.getElementById('canvas'), location.origin, proof);
  status.hidden = true;
  window.addEventListener('pagehide', () => app.destroy());
}} catch (_) {{
  status.textContent = 'NW_BOOT_FAILED: The workspace could not start. Open it again from the local application or reload.';
}}
"#);
    let mut boot=asset(&output,bootstrap.as_bytes(),"js","application/javascript")?;
    boot["immutable"]=serde_json::json!(false);
    let css=asset(&output,b"html,body{margin:0;width:100%;height:100%;overflow:hidden;background:#111;color:#ddd;font:15px system-ui}canvas{display:block;width:100%;height:100%}#boot-status{position:fixed;inset:40% 12%;text-align:center;pointer-events:none}","css","text/css")?;
    let index=format!(r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Numera Wiz</title><link rel="stylesheet" href="/{}"></head><body><canvas id="canvas"></canvas><div id="boot-status">Connecting to the local workspace…</div><script type="module" src="/{}"></script></body></html>"#,css["path"].as_str().context("css path")?,boot["path"].as_str().context("bootstrap path")?);
    fs::write(output.join("index.html"),index.as_bytes())?;
    let index_entry=serde_json::json!({"path":"index.html","sha256":hex::encode(Sha256::digest(index.as_bytes())),"bytes":index.len(),"mime":"text/html; charset=utf-8","immutable":false});
    fs::write(output.join("manifest.json"),serde_json::to_vec_pretty(&serde_json::json!({"schema_version":1,"source":source,"registry":nw_domain::commands::registry_fingerprint(),"tool":tool,"assets":[index_entry,wasm,js,boot,css]}))?)?;
    tracing::info!(output=%output.display(),"Built same-source WASM assets and executable namespace manifest"); Ok(())
}

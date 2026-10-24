//! Shared hosted owner/CLI fixture. Never executed by local source validation.
use nw_domain::commands::Request;
use serde_json::{Value, json};
use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::io::AsyncWriteExt;

pub struct Cli {
    pub data: PathBuf,
    pub components: PathBuf,
    pub request_timeout: Duration,
}
impl Cli {
    pub async fn human(&self, args: &[&str]) -> (Value, i32) {
        let child = self
            .command()
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let output = tokio::time::timeout(self.request_timeout.max(Duration::from_secs(45)), child.wait_with_output())
            .await
            .unwrap()
            .unwrap();
        let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
            panic!(
                "Human CLI failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )
        });
        (value, output.status.code().unwrap())
    }
    pub fn command(&self) -> tokio::process::Command {
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_numera-wiz"));
        command
            .arg("--data-dir")
            .arg(&self.data)
            .arg("--components-dir")
            .arg(&self.components);
        command
    }
    pub async fn raw(&self, request: &Request) -> Value {
        self.raw_with_exit(request).await.0
    }
    pub async fn raw_with_exit(&self, request: &Request) -> (Value, i32) {
        let mut child = self
            .command()
            .arg("request")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut input = child.stdin.take().unwrap();
        input
            .write_all(&serde_json::to_vec(request).unwrap())
            .await
            .unwrap();
        drop(input);
        let output = tokio::time::timeout(self.request_timeout, child.wait_with_output())
            .await
            .unwrap()
            .unwrap();
        let value = serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|_| panic!("CLI failed: {}", String::from_utf8_lossy(&output.stderr)));
        (
            value,
            output
                .status
                .code()
                .expect("CLI must report its synchronous exit code"),
        )
    }
    pub async fn query(&self, id: &str, params: Value) -> Value {
        let response = self.raw(&Request::query(id, params)).await;
        assert_eq!(response["ok"], true, "{response}");
        response["result"].clone()
    }
    pub async fn mutation(&self, scope: &str, key: &str, id: &str, params: Value) -> Value {
        let mut request = Request::query(id, params);
        request.replay_scope = Some(scope.into());
        request.idempotency_key = Some(key.into());
        self.raw(&request).await
    }
    pub async fn completed(&self, id: &str) -> Value {
        for _ in 0..100 {
            let job = self.query("jobs.inspect", json!({"id":id})).await;
            if ["succeeded", "failed", "cancelled", "interrupted", "skipped"]
                .contains(&job["state"].as_str().unwrap())
            {
                return job;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("job completion deadline exceeded");
    }
}

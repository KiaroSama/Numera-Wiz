//! Bounded transport for two prepared NUT streams, before output verification.
use crate::Engine;
use nw_domain::{DomainError, Result};
use nw_platform::{
    files::DirectoryStamp,
    process::{
        ByteInput, InvocationSpec, MediaProcessSession, PreparedRelays, RelayInputId, SessionArg,
        SessionOutput, SessionPolicy, StdoutMode,
    },
};
use tokio_util::sync::CancellationToken;

impl Engine {
    /// Mux prepared video and audio transports into a bounded candidate. The
    /// planner must supply the exact held transport inputs. The caller must
    /// verify the candidate's decoded media and timestamps before publication.
    pub async fn mux_normalized_pair_candidate(
        &self,
        inputs: [ByteInput; 2],
        private_root: DirectoryStamp,
        cancel: CancellationToken,
    ) -> Result<SessionOutput> {
        let mut executable_sha256 = [0; 32];
        hex::decode_to_slice(&self.ffmpeg.sha256, &mut executable_sha256)
            .map_err(|_| DomainError::new("COMPONENT_UNAVAILABLE", "Invalid FFmpeg identity"))?;
        let mut args = Vec::from(literal_args([
            "-hide_banner",
            "-nostdin",
            "-v",
            "error",
            "-xerror",
            "-max_alloc",
            "67108864",
            "-copyts",
        ]));
        for index in 0..2 {
            args.extend(literal_args([
                "-protocol_whitelist",
                "file",
                "-format_whitelist",
                "nut",
                "-f",
                "nut",
                "-i",
            ]));
            args.push(SessionArg::Input(RelayInputId(index)));
        }
        args.extend(literal_args([
            "-map",
            "0:v:0",
            "-map",
            "1:a:0",
            "-map_metadata",
            "-1",
            "-map_chapters",
            "-1",
            "-c",
            "copy",
            "-avoid_negative_ts",
            "disabled",
            "-f",
            "nut",
            "pipe:1",
        ]));
        let spec = InvocationSpec {
            program: self.ffmpeg.path.clone().into(),
            args,
            executable_sha256,
            profile_id: "normalized-nut-pair-v1".into(),
        };
        let ticket = self.process_budget.acquire(&cancel).await?;
        let prepared = PreparedRelays::prepare(
            inputs,
            private_root,
            SessionPolicy::byte_fixture(),
            Box::new(ticket),
            cancel,
        )
        .await
        .map_err(|failure| failure.cause)?;
        let session =
            MediaProcessSession::spawn(prepared, spec, StdoutMode::Capture { limit: 256 * 1024 })
                .await
                .map_err(|failure| failure.cause)?;
        let output = session.finish().await.map_err(|failure| failure.cause)?;
        if output.output.stdout.is_empty()
            || !output
                .closed
                .pumps()
                .iter()
                .all(|pump| pump.transport_complete())
        {
            return Err(DomainError::new(
                "PROCESS_OUTPUT_REJECTED",
                "Incomplete normalized pair output",
            ));
        }
        Ok(output)
    }
}

fn literal_args<const N: usize>(values: [&str; N]) -> [SessionArg; N] {
    values.map(|value| SessionArg::Literal(value.into()))
}

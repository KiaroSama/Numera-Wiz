# Your first export

Download the standalone CPU package for your platform. Extract it to a folder you can write to. The development builds are unsigned or ad-hoc signed. Use a state directory on ext4 (Linux), APFS (macOS) or NTFS (Windows). [Build artifacts](https://github.com/KiaroSama/Numera-Wiz/actions/workflows/ci.yml)

The sample is a one-second synthetic video with four frames. The walkthrough registers the input, reviews the exact H.264 plan, submits that plan and saves a second copy using the artifact's verified SHA-256. Keep the returned IDs if you want to inspect or retry the job later. For an older package without `examples/quickstart`, save the sample to that path under the package's resources directory. [Sample video](../../examples/quickstart/clip.mkv)

## Linux and macOS

Run these commands from the extracted package directory. The examples use Bash and `jq`. On macOS use the application bundle path in the second pair of assignments.

```bash
app="$PWD/numera-wiz"
resources="$PWD"
# macOS
# app="$PWD/Numera Wiz.app/Contents/MacOS/numera-wiz"
# resources="$PWD/Numera Wiz.app/Contents/Resources"

work=$(mktemp -d "$PWD/first-export.XXXXXX")
nw() { "$app" --data-dir "$work/state" "$@"; }

scope=$(nw access replay-scope create --json | jq -er '.result.replay_scope')
asset=$(nw assets register "$resources/examples/quickstart/clip.mkv" \
  --replay-scope "$scope" --idempotency-key quickstart-register | jq -er '.result.id')
jq -n --arg asset "$asset" --arg out "$work/output.mp4" '{
  asset_id: $asset, output: $out,
  settings: {video_stream: 0, container: "mp4", crf: 18, preset: "balanced", audio: {kind: "drop"}}
}' > "$work/encode.json"
plan=$(nw media encode --file "$work/encode.json" | jq -er '.result.id')
nw plans inspect "$plan" > "$work/review.json"
cat "$work/review.json"
hash=$(jq -er '.result.hash' "$work/review.json")
```

After reviewing the source, output, selected stream and plan hash, submit the job.

```bash
job=$(nw jobs submit --plan "$plan" --plan-hash "$hash" \
  --replay-scope "$scope" --idempotency-key quickstart-export | jq -er '.result.job_id')
nw jobs wait "$job" --timeout-seconds 60
artifact="job-$job"
digest=$(nw artifacts manifest --artifact-id "$artifact" | jq -er '.result.sha256')
nw artifacts save --artifact-id "$artifact" --expected-sha256 "$digest" --out "$work/verified.mp4"
```

## Windows PowerShell 7

Run these commands from the extracted package directory, such as `G:\Program Files\Numera Wiz`. The example keeps job data in the writable `G:\Program Files\Portable\Scripts` directory. Choose another writable NTFS location if your installation uses a different drive.

```powershell
$app = Join-Path $PWD 'numera-wiz.exe'
$fixture = Join-Path $PWD 'examples\quickstart\clip.mkv'
$work = Join-Path 'G:\Program Files\Portable\Scripts' ('Numera Wiz ' + [guid]::NewGuid())
New-Item -ItemType Directory -Path $work | Out-Null
function nw {
  $output = & $app --data-dir (Join-Path $work 'state') @args
  if ($LASTEXITCODE -ne 0) { throw ($output -join "`n") }
  $output | ConvertFrom-Json
}

$scope = (nw access replay-scope create --json).result.replay_scope
$asset = (nw assets register $fixture --replay-scope $scope --idempotency-key quickstart-register).result.id
$request = Join-Path $work 'encode.json'
@{
  asset_id = $asset
  output = Join-Path $work 'output.mp4'
  settings = @{ video_stream = 0; container = 'mp4'; crf = 18; preset = 'balanced'; audio = @{ kind = 'drop' } }
} | ConvertTo-Json -Depth 6 | Set-Content -Encoding utf8 $request
$plan = (nw media encode --file $request).result.id
$review = (nw plans inspect $plan).result
$review | ConvertTo-Json -Depth 20
```

After reviewing the source, output, selected stream and plan hash, submit the job.

```powershell
$job = (nw jobs submit --plan $plan --plan-hash $review.hash --replay-scope $scope --idempotency-key quickstart-export).result.job_id
nw jobs wait $job --timeout-seconds 60
$artifact = 'job-' + $job
$digest = (nw artifacts manifest --artifact-id $artifact).result.sha256
nw artifacts save --artifact-id $artifact --expected-sha256 $digest --out (Join-Path $work 'verified.mp4')
```

Open `verified.mp4` in a video player. If the submit response was lost, reuse the same plan ID, hash, replay scope and idempotency key.

For the same workflow in an agent, follow [agent setup](../agents/README.md) to connect through stdio or loopback HTTP. Read the selected tool schema first. Grant read access to the sample and write access to the new output directory before registering or submitting media.

param(
    [Parameter(Mandatory = $true)][string]$FFmpeg,
    [Parameter(Mandatory = $true)][string]$Needle,
    [Parameter(Mandatory = $true)][string]$Model,
    [string]$WorkDirectory = (Join-Path $env:RUNNER_TEMP 'storyteller-whistle-smoke'),
    [switch]$NativeOnly
)

$ErrorActionPreference = 'Stop'
$env:NEEDLE_TELEMETRY = '0'
$env:DO_NOT_TRACK = '1'
New-Item -ItemType Directory -Path $WorkDirectory -Force | Out-Null
Add-Type -AssemblyName System.Speech
$speech = Join-Path $WorkDirectory 'speech.wav'
$synthesizer = New-Object System.Speech.Synthesis.SpeechSynthesizer
try {
    $synthesizer.SetOutputToWaveFile($speech)
    $synthesizer.Speak('The lighthouse stands beside the quiet harbor. A small boat returns home before sunset.')
} finally {
    $synthesizer.Dispose()
}
$short = Join-Path $WorkDirectory 'short.wav'
& $FFmpeg -hide_banner -loglevel error -y -i $speech -ar 16000 -ac 1 -c:a pcm_s16le $short
if ($LASTEXITCODE -ne 0) { throw 'Could not normalize the speech fixture.' }

$raw = (& $Needle --model $Model --audio $short --audio-word-timestamps --audio-language en) -join "`n"
if ($LASTEXITCODE -ne 0) { throw 'Native Whistle speech invocation failed.' }
$raw | Set-Content -LiteralPath (Join-Path $WorkDirectory 'native-speech.json') -Encoding UTF8
$result = $raw | ConvertFrom-Json
if ([string]::IsNullOrWhiteSpace($result.text) -or $result.language -ne 'en' -or @($result.words).Count -eq 0) {
    throw 'Whistle did not return English speech with native word timestamps.'
}
Write-Host "Whistle native speech: $($result.text)"
Write-Host "Native timed words: $(@($result.words).Count)"

$silence = Join-Path $WorkDirectory 'silence.wav'
& $FFmpeg -hide_banner -loglevel error -y -f lavfi -i 'anullsrc=r=16000:cl=mono' -t 1 -c:a pcm_s16le $silence
if ($LASTEXITCODE -ne 0) { throw 'Could not generate the silence fixture.' }
$rawSilence = (& $Needle --model $Model --audio $silence --audio-word-timestamps) -join "`n"
if ($LASTEXITCODE -ne 0) { throw 'Native Whistle silence invocation failed.' }
$silent = $rawSilence | ConvertFrom-Json
if (-not [string]::IsNullOrWhiteSpace($silent.text) -or @($silent.words).Count -ne 0) {
    throw 'Whistle invented speech for silence.'
}
if ($NativeOnly) { return }

$shortOutput = Join-Path $WorkDirectory 'short-output'
& cargo run --locked -p storyteller-application --example transcribe_whistle -- $short $shortOutput $FFmpeg $Needle $Model 1 en
if ($LASTEXITCODE -ne 0) { throw 'Application Whistle adapter failed on short speech.' }
$shortTranscript = Get-Content -LiteralPath (Join-Path $shortOutput 'transcript.json') -Raw | ConvertFrom-Json
if (@($shortTranscript.segments).Count -eq 0) { throw 'Short speech produced no normalized phrases.' }

$long = Join-Path $WorkDirectory 'long.wav'
& $FFmpeg -hide_banner -loglevel error -y -stream_loop -1 -i $short -t 55 -ar 16000 -ac 1 -c:a pcm_s16le $long
if ($LASTEXITCODE -ne 0) { throw 'Could not generate multi-window speech.' }
$longOutput = Join-Path $WorkDirectory 'long-output'
& cargo run --locked -p storyteller-application --example transcribe_whistle -- $long $longOutput $FFmpeg $Needle $Model 2 auto
if ($LASTEXITCODE -ne 0) { throw 'Application Whistle adapter failed on multi-window speech.' }
$plan = Get-Content -LiteralPath (Join-Path $longOutput 'transcription-plan.json') -Raw | ConvertFrom-Json
if (@($plan.chunks).Count -lt 2 -or $plan.duration_ms -lt 54000) { throw 'The application did not split long audio.' }
$cursor = 0
foreach ($chunk in $plan.chunks) {
    if ($chunk.start_ms -ne $cursor -or $chunk.end_ms -le $cursor -or $chunk.end_ms - $chunk.start_ms -gt 30000) {
        throw 'Whistle chunk plan has a gap, overlap, empty window or oversized window.'
    }
    $cursor = $chunk.end_ms
}
if ($cursor -ne $plan.duration_ms) { throw 'Whistle chunk plan lost the audio tail.' }
$transcript = Get-Content -LiteralPath (Join-Path $longOutput 'transcript.json') -Raw | ConvertFrom-Json
$previousEnd = 0
foreach ($segment in $transcript.segments) {
    if ($segment.start_ms -lt $previousEnd -or $segment.end_ms -le $segment.start_ms -or $segment.end_ms -gt $plan.duration_ms) {
        throw 'Normalized Whistle timing overlaps or exceeds the source.'
    }
    $previousEnd = $segment.end_ms
}
if ($previousEnd -le 30000) { throw 'Merged Whistle speech lost later chunks or their offsets.' }
if (Test-Path -LiteralPath (Join-Path $longOutput 'transcription-chunks.tmp')) { throw 'Temporary PCM chunks were not removed.' }

$silentOutput = Join-Path $WorkDirectory 'silent-output'
& cargo run --locked -p storyteller-application --example transcribe_whistle -- $silence $silentOutput $FFmpeg $Needle $Model 1 auto
if ($LASTEXITCODE -eq 0) { throw 'An entirely silent book was incorrectly accepted.' }
if (Test-Path -LiteralPath (Join-Path $silentOutput 'transcript.json')) { throw 'Silence produced a fabricated transcript.' }
if (Test-Path -LiteralPath (Join-Path $silentOutput 'transcription-chunks.tmp')) { throw 'Failed analysis left temporary PCM chunks.' }
Write-Host 'Whistle native and application smoke passed.'

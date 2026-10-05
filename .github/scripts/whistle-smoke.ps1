param(
    [Parameter(Mandatory = $true)][string]$FFmpeg,
    [Parameter(Mandatory = $true)][string]$Needle,
    [Parameter(Mandatory = $true)][string]$Model,
    [string]$WorkDirectory = (Join-Path $env:RUNNER_TEMP 'storyteller-whistle-smoke'),
    [switch]$NativeOnly,
    [switch]$ExpandedWorkers
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
$rawSilence = (& $Needle --model $Model --audio $silence --audio-word-timestamps --audio-language en) -join "`n"
if ($LASTEXITCODE -ne 0) { throw 'Native Whistle silence invocation failed.' }
$silent = $rawSilence | ConvertFrom-Json
if (-not [string]::IsNullOrWhiteSpace($silent.text) -or @($silent.words).Count -ne 0) {
    throw 'Whistle invented speech for silence.'
}
if ($NativeOnly) { return }

$shortOutput = Join-Path $WorkDirectory 'short-output'
& cargo run --locked -p storyteller-application --example transcribe_whistle -- $short $shortOutput $FFmpeg $Needle $Model 1
if ($LASTEXITCODE -ne 0) { throw 'Application Whistle adapter failed on short speech.' }
$shortTranscript = Get-Content -LiteralPath (Join-Path $shortOutput 'transcript.json') -Raw | ConvertFrom-Json
if (@($shortTranscript.segments).Count -eq 0) { throw 'Short speech produced no normalized phrases.' }
if ($shortTranscript.language -ne 'en') { throw 'Short speech did not preserve the English-only contract.' }
if ($ExpandedWorkers) {
    $automaticOutput = Join-Path $WorkDirectory 'automatic-output'
    & cargo run --locked -p storyteller-application --example transcribe_whistle -- $short $automaticOutput $FFmpeg $Needle $Model auto
    if ($LASTEXITCODE -ne 0) { throw 'Automatic system-based Whistle worker selection failed.' }
}

$long = Join-Path $WorkDirectory 'long.wav'
$longDuration = if ($ExpandedWorkers) { 135 } else { 55 }
$workerCount = if ($ExpandedWorkers) { 8 } else { 2 }
& $FFmpeg -hide_banner -loglevel error -y -stream_loop -1 -i $short -t $longDuration -ar 16000 -ac 1 -c:a pcm_s16le $long
if ($LASTEXITCODE -ne 0) { throw 'Could not generate multi-window speech.' }
$longOutput = Join-Path $WorkDirectory 'long-output'
$applicationOutput = (& cargo run --locked -p storyteller-application --example transcribe_whistle -- $long $longOutput $FFmpeg $Needle $Model $workerCount) -join "`n"
if ($LASTEXITCODE -ne 0) { throw 'Application Whistle adapter failed on multi-window speech.' }
$plan = Get-Content -LiteralPath (Join-Path $longOutput 'transcription-plan.json') -Raw | ConvertFrom-Json
if (@($plan.chunks).Count -lt 2 -or $plan.duration_ms -lt ($longDuration * 1000 - 1000)) { throw 'The application did not split long audio.' }
if ($ExpandedWorkers) {
    $effective = [Math]::Min($workerCount, @($plan.chunks).Count)
    if ($effective -le 4 -or $applicationOutput -notmatch "with $effective workers") { throw 'Expanded worker smoke did not exercise more than four native workers.' }
    Write-Host "Expanded worker smoke: $effective native workers across $(@($plan.chunks).Count) chunks."
}
Write-Host $applicationOutput
$cursor = 0
foreach ($chunk in $plan.chunks) {
    if ($chunk.start_ms -ne $cursor -or $chunk.end_ms -le $cursor -or $chunk.end_ms - $chunk.start_ms -gt 27000) {
        throw 'Whistle chunk plan has a gap, overlap, empty window or oversized window.'
    }
    $cursor = $chunk.end_ms
}
if ($cursor -ne $plan.duration_ms) { throw 'Whistle chunk plan lost the audio tail.' }
if (@($plan.inference_windows).Count -ne @($plan.chunks).Count) { throw 'Missing contextual inference windows.' }
foreach ($window in $plan.inference_windows) {
    if ($window.start_ms -gt $window.owned.start_ms -or $window.end_ms -lt $window.owned.end_ms -or $window.end_ms - $window.start_ms -gt 30000 -or $window.end_ms -gt $plan.duration_ms) {
        throw 'Contextual input exceeds 30 seconds or fails to cover its ownership range.'
    }
}
$transcript = Get-Content -LiteralPath (Join-Path $longOutput 'transcript.json') -Raw | ConvertFrom-Json
if ($transcript.language -ne 'en') { throw 'Multi-window speech did not preserve the English-only contract.' }
$previousEnd = 0
foreach ($segment in $transcript.segments) {
    if ($segment.start_ms -lt $previousEnd -or $segment.end_ms -le $segment.start_ms -or $segment.end_ms -gt $plan.duration_ms) {
        throw 'Normalized Whistle timing overlaps or exceeds the source.'
    }
    $previousEnd = $segment.end_ms
}
if ($previousEnd -le 30000) { throw 'Merged Whistle speech lost later chunks or their offsets.' }
if (Test-Path -LiteralPath (Join-Path $longOutput 'transcription-chunks.tmp')) { throw 'Temporary PCM chunks were not removed.' }

# Place a chapter cut inside a uniquely decoded word's native baseline interval.
# The shifted source preserves the complete word in both contextual inputs.
$target = $null
$targetKey = ''
foreach ($word in $result.words) {
    $key = ([regex]::Replace($word.word, '[^\p{L}\p{N}]', '')).ToLowerInvariant()
    $matchingWords = @($result.words | Where-Object { ([regex]::Replace($_.word, '[^\p{L}\p{N}]', '')).ToLowerInvariant() -eq $key })
    if ($key.Length -ge 8 -and $matchingWords.Count -eq 1 -and $word.end -gt $word.start -and $word.end -lt 3) {
        $target = $word
        $targetKey = $key
        break
    }
}
if ($null -eq $target) { throw 'No unique early word with usable timing in the boundary fixture.' }
$cut = 24500
$wordStart = [long][Math]::Round($target.start * 1000)
$wordEnd = [long][Math]::Round($target.end * 1000)
$padding = $cut - [long][Math]::Floor(($wordStart + $wordEnd) / 2)
$padded = Join-Path $WorkDirectory 'boundary.wav'
& $FFmpeg -hide_banner -loglevel error -y -i $short -af "adelay=${padding}:all=1,apad" -t 31 -ar 16000 -ac 1 -c:a pcm_s16le $padded
if ($LASTEXITCODE -ne 0) { throw 'Could not shift speech across the boundary.' }
$metadata = Join-Path $WorkDirectory 'boundary.ffmetadata'
$metadataText = ";FFMETADATA1`n[CHAPTER]`nTIMEBASE=1/1000`nSTART=0`nEND=$cut`ntitle=Word boundary`n"
[System.IO.File]::WriteAllText($metadata, $metadataText, (New-Object System.Text.UTF8Encoding($false)))
$chaptered = Join-Path $WorkDirectory 'boundary.mka'
& $FFmpeg -hide_banner -loglevel error -y -i $padded -f ffmetadata -i $metadata -map 0:a -map_metadata 1 -map_chapters 1 -c:a copy $chaptered
if ($LASTEXITCODE -ne 0) { throw 'Could not preserve the forced word-boundary chapter cut.' }
$boundaryOutput = Join-Path $WorkDirectory 'boundary-output'
& cargo run --locked -p storyteller-application --example transcribe_whistle -- $chaptered $boundaryOutput $FFmpeg $Needle $Model 2
if ($LASTEXITCODE -ne 0) { throw 'Contextual Whistle adapter failed at the forced word boundary.' }
$boundaryPlan = Get-Content -LiteralPath (Join-Path $boundaryOutput 'transcription-plan.json') -Raw | ConvertFrom-Json
if ($boundaryPlan.chunks[0].end_ms -ne $cut -or @($boundaryPlan.inference_windows).Count -ne 2) { throw 'Boundary fixture did not use the requested cut and two inputs.' }
$shiftedStart = $wordStart + $padding
$shiftedEnd = $wordEnd + $padding
if ($shiftedStart -ge $cut -or $shiftedEnd -le $cut) { throw 'Native baseline word does not straddle the cut.' }
foreach ($window in $boundaryPlan.inference_windows) {
    if ($window.start_ms -gt $shiftedStart -or $window.end_ms -lt $shiftedEnd -or $window.end_ms - $window.start_ms -gt 30000) { throw 'Both bounded inputs must include the complete boundary word.' }
}
$boundaryTranscript = Get-Content -LiteralPath (Join-Path $boundaryOutput 'transcript.json') -Raw | ConvertFrom-Json
$boundaryText = (@($boundaryTranscript.segments | ForEach-Object { $_.text })) -join ' '
$occurrences = @($boundaryText -split '\s+' | Where-Object { ([regex]::Replace($_, '[^\p{L}\p{N}]', '')).ToLowerInvariant() -eq $targetKey }).Count
if ($occurrences -ne 1) { throw "Boundary word '$targetKey' must appear once; found $occurrences in '$boundaryText'." }
if (Test-Path -LiteralPath (Join-Path $boundaryOutput 'transcription-chunks.tmp')) { throw 'Boundary inference left temporary PCM inputs.' }
[ordered]@{
    engine = 'Whistle native CPU'
    target = $targetKey
    baseline_start_ms = $wordStart
    baseline_end_ms = $wordEnd
    source_word_start_ms = $shiftedStart
    source_word_end_ms = $shiftedEnd
    cut_ms = $cut
    target_occurrences = $occurrences
    transcript = $boundaryText
    inference_windows = $boundaryPlan.inference_windows
} | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $WorkDirectory 'boundary-evidence.json') -Encoding UTF8
Write-Host "Native Whistle word boundary retained once: $targetKey across $cut ms."

$silentOutput = Join-Path $WorkDirectory 'silent-output'
& cargo run --locked -p storyteller-application --example transcribe_whistle -- $silence $silentOutput $FFmpeg $Needle $Model 1
if ($LASTEXITCODE -eq 0) { throw 'An entirely silent book was incorrectly accepted.' }
if (Test-Path -LiteralPath (Join-Path $silentOutput 'transcript.json')) { throw 'Silence produced a fabricated transcript.' }
if (Test-Path -LiteralPath (Join-Path $silentOutput 'transcription-chunks.tmp')) { throw 'Failed analysis left temporary PCM chunks.' }
$global:LASTEXITCODE = 0
Write-Host 'Whistle native and application smoke passed.'

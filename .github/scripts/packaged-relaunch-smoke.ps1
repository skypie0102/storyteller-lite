param(
    [Parameter(Mandatory = $true)]
    [string]$Executable
)

$ErrorActionPreference = 'Stop'

$resolvedExecutable = (Resolve-Path -LiteralPath $Executable).Path
$smokeRoot = Join-Path $env:RUNNER_TEMP ("storyteller-packaged-relaunch-" + [guid]::NewGuid().ToString('N'))
$localAppData = Join-Path $smokeRoot 'localappdata'
$appData = Join-Path $localAppData 'Storyteller OneClick Lite'
$recoveryPath = Join-Path $appData 'queue-recovery.json'
$stdoutPath = Join-Path $smokeRoot 'storyteller.stdout.log'
$stderrPath = Join-Path $smokeRoot 'storyteller.stderr.log'
$runningJobId = '11111111-1111-4111-8111-111111111111'
$reviewJobId = '22222222-2222-4222-8222-222222222222'

New-Item -ItemType Directory -Force $appData | Out-Null

$seed = [ordered]@{
    version = 1
    jobs = @(
        [ordered]@{
            id = $runningJobId
            title = 'Packaged running recovery smoke'
            epub_path = (Join-Path $smokeRoot 'missing-running.epub')
            audiobook_path = (Join-Path $smokeRoot 'missing-running.m4b')
            output_path = (Join-Path $smokeRoot 'running-output.epub')
            audio_codec = 'opus'
            audio_bitrate_kbps = 64
            language = $null
            whisper_model = 'large-v3-turbo'
            audio_review_policy = 'smart'
            whisper_workers = 1
            previous_status = 'running'
            checkpoints = @()
        },
        [ordered]@{
            id = $reviewJobId
            title = 'Packaged review recovery smoke'
            epub_path = (Join-Path $smokeRoot 'missing-review.epub')
            audiobook_path = (Join-Path $smokeRoot 'missing-review.m4b')
            output_path = (Join-Path $smokeRoot 'review-output.epub')
            audio_codec = 'opus'
            audio_bitrate_kbps = 64
            language = $null
            whisper_model = 'large-v3-turbo'
            audio_review_policy = 'smart'
            whisper_workers = 1
            previous_status = 'needs_review'
            checkpoints = @(
                [ordered]@{ stage = 'prepare'; fingerprint = 'prepare-smoke' },
                [ordered]@{ stage = 'analyze'; fingerprint = 'analyze-smoke' },
                [ordered]@{ stage = 'align'; fingerprint = 'align-smoke' },
                [ordered]@{ stage = 'review_audio'; fingerprint = 'review-smoke' }
            )
        }
    )
}
$seed | ConvertTo-Json -Depth 8 | Set-Content -Encoding UTF8 $recoveryPath

$previousLocalAppData = $env:LOCALAPPDATA
$previousSlintBackend = $env:SLINT_BACKEND
$process = $null
try {
    $env:LOCALAPPDATA = $localAppData
    # Hosted Windows runners do not provide a reliable GPU/OpenGL surface. Slint's
    # production Winit backend supports a software renderer, which keeps the smoke
    # focused on packaged startup/recovery rather than runner graphics capabilities.
    $env:SLINT_BACKEND = 'winit-software'
    $process = Start-Process `
        -FilePath $resolvedExecutable `
        -WorkingDirectory (Split-Path $resolvedExecutable) `
        -RedirectStandardOutput $stdoutPath `
        -RedirectStandardError $stderrPath `
        -PassThru

    Start-Sleep -Seconds 6
    $process.Refresh()
    if ($process.HasExited) {
        $stdout = if (Test-Path -LiteralPath $stdoutPath) { (Get-Content -LiteralPath $stdoutPath -Raw).Trim() } else { '' }
        $stderr = if (Test-Path -LiteralPath $stderrPath) { (Get-Content -LiteralPath $stderrPath -Raw).Trim() } else { '' }
        throw "Packaged StoryTeller Lite exited during the relaunch smoke test with code $($process.ExitCode). stdout=[$stdout] stderr=[$stderr]"
    }
    if (-not (Test-Path -LiteralPath $recoveryPath -PathType Leaf)) {
        throw 'The packaged app removed the seeded recovery file instead of preserving paused work.'
    }

    $restored = Get-Content -LiteralPath $recoveryPath -Raw | ConvertFrom-Json
    $jobs = @($restored.jobs)
    if ($restored.version -ne 1) {
        throw "Unexpected queue recovery version after packaged launch: $($restored.version)."
    }
    if ($jobs.Count -ne 2) {
        throw "Expected exactly two recovered jobs after packaged launch; found $($jobs.Count)."
    }

    $running = @($jobs | Where-Object { $_.id -eq $runningJobId })
    if ($running.Count -ne 1) {
        throw "Expected exactly one restored Running smoke job; found $($running.Count)."
    }
    if ($running[0].previous_status -ne 'waiting') {
        throw "Interrupted packaged work did not restore as waiting; persisted status is $($running[0].previous_status)."
    }

    $review = @($jobs | Where-Object { $_.id -eq $reviewJobId })
    if ($review.Count -ne 1) {
        throw "Expected exactly one restored NeedsReview smoke job; found $($review.Count)."
    }
    if ($review[0].previous_status -ne 'waiting') {
        throw "Packaged NeedsReview work did not restore as waiting; persisted status is $($review[0].previous_status)."
    }
    $reviewStages = @($review[0].checkpoints | ForEach-Object { $_.stage })
    $expectedReviewStages = @('prepare', 'analyze', 'align')
    if ($reviewStages.Count -ne $expectedReviewStages.Count) {
        throw "NeedsReview rewind kept an unexpected checkpoint count: $($reviewStages.Count)."
    }
    for ($index = 0; $index -lt $expectedReviewStages.Count; $index++) {
        if ($reviewStages[$index] -ne $expectedReviewStages[$index]) {
            throw "NeedsReview rewind checkpoint $index is '$($reviewStages[$index])'; expected '$($expectedReviewStages[$index])'."
        }
    }
    if ($reviewStages -contains 'review_audio') {
        throw 'NeedsReview rewind retained the Review Audio checkpoint instead of forcing review to rerun.'
    }

    $quarantined = @(Get-ChildItem -LiteralPath $appData -Filter 'queue-recovery.invalid-*' -File -ErrorAction SilentlyContinue)
    if ($quarantined.Count -ne 0) {
        throw 'A valid packaged recovery snapshot was unexpectedly quarantined.'
    }

    Write-Host 'Packaged relaunch recovery smoke test passed: Running restored as waiting, NeedsReview rewound before Review Audio, and recovered work remained paused.'
}
finally {
    if ($null -ne $process) {
        $process.Refresh()
        if (-not $process.HasExited) {
            Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
            Wait-Process -Id $process.Id -ErrorAction SilentlyContinue
        }
    }

    if ($null -eq $previousLocalAppData) {
        Remove-Item Env:LOCALAPPDATA -ErrorAction SilentlyContinue
    }
    else {
        $env:LOCALAPPDATA = $previousLocalAppData
    }
    if ($null -eq $previousSlintBackend) {
        Remove-Item Env:SLINT_BACKEND -ErrorAction SilentlyContinue
    }
    else {
        $env:SLINT_BACKEND = $previousSlintBackend
    }
    Remove-Item -LiteralPath $smokeRoot -Recurse -Force -ErrorAction SilentlyContinue
}

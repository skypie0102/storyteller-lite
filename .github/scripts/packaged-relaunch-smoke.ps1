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
$jobId = '11111111-1111-4111-8111-111111111111'

New-Item -ItemType Directory -Force $appData | Out-Null

$seed = [ordered]@{
    version = 1
    jobs = @(
        [ordered]@{
            id = $jobId
            title = 'Packaged recovery smoke'
            epub_path = (Join-Path $smokeRoot 'missing-source.epub')
            audiobook_path = (Join-Path $smokeRoot 'missing-source.m4b')
            output_path = (Join-Path $smokeRoot 'smoke-output.epub')
            audio_codec = 'opus'
            audio_bitrate_kbps = 64
            language = $null
            whisper_model = 'large-v3-turbo'
            audio_review_policy = 'smart'
            whisper_workers = 1
            previous_status = 'running'
            checkpoints = @()
        }
    )
}
$seed | ConvertTo-Json -Depth 8 | Set-Content -Encoding UTF8 $recoveryPath

$previousLocalAppData = $env:LOCALAPPDATA
$process = $null
try {
    $env:LOCALAPPDATA = $localAppData
    $process = Start-Process -FilePath $resolvedExecutable -WorkingDirectory (Split-Path $resolvedExecutable) -PassThru

    Start-Sleep -Seconds 6
    $process.Refresh()
    if ($process.HasExited) {
        throw "Packaged StoryTeller Lite exited during the relaunch smoke test with code $($process.ExitCode)."
    }
    if (-not (Test-Path -LiteralPath $recoveryPath -PathType Leaf)) {
        throw 'The packaged app removed the seeded recovery file instead of preserving paused work.'
    }

    $restored = Get-Content -LiteralPath $recoveryPath -Raw | ConvertFrom-Json
    $jobs = @($restored.jobs)
    if ($restored.version -ne 1) {
        throw "Unexpected queue recovery version after packaged launch: $($restored.version)."
    }
    if ($jobs.Count -ne 1) {
        throw "Expected exactly one recovered job after packaged launch; found $($jobs.Count)."
    }
    if ($jobs[0].id -ne $jobId) {
        throw "Recovered job identity changed during packaged launch: $($jobs[0].id)."
    }
    if ($jobs[0].previous_status -ne 'waiting') {
        throw "Interrupted packaged work did not restore as waiting; persisted status is $($jobs[0].previous_status)."
    }

    $quarantined = @(Get-ChildItem -LiteralPath $appData -Filter 'queue-recovery.invalid-*' -File -ErrorAction SilentlyContinue)
    if ($quarantined.Count -ne 0) {
        throw 'A valid packaged recovery snapshot was unexpectedly quarantined.'
    }

    Write-Host 'Packaged relaunch recovery smoke test passed: interrupted work restored as waiting and remained paused.'
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
    Remove-Item -LiteralPath $smokeRoot -Recurse -Force -ErrorAction SilentlyContinue
}

from pathlib import Path


def replace_exact(path: str, old: str, new: str, expected: int = 1) -> None:
    file_path = Path(path)
    text = file_path.read_text(encoding="utf-8")
    count = text.count(old)
    if count != expected:
        raise SystemExit(f"{path}: expected {expected} occurrences, found {count}: {old!r}")
    file_path.write_text(text.replace(old, new), encoding="utf-8")


path = "crates/storyteller-ui/src/runtime_setup.rs"

replace_exact(
    path,
    '''const WHISPER_RELEASES_API: &str =
    "https://api.github.com/repos/ggml-org/whisper.cpp/releases?per_page=10";
''',
    '''const WHISPER_BUILD_TAG: &str = "b5130";
const WHISPER_DOWNLOAD_BASE: &str = "https://github.com/ggml-org/whisper.cpp/releases/download";
const WHISPER_CPU_ASSET: &str = "whisper-bin-x64.zip";
const WHISPER_CPU_SHA256: &str =
    "f9ec6c52a2e949b62ab51fa21d0d497958f9e41c3010c157c4e42932d5316f3c";
const WHISPER_CUDA_ASSET: &str = "whisper-cublas-12.4.0-bin-x64.zip";
const WHISPER_CUDA_SHA256: &str =
    "af520ddd034d985b55dfeea3e465ed93653ba2aee1a55e865033edc548c272a7";
''',
)

replace_exact(
    path,
    '''                progress(if prefer_cuda {
                    "NVIDIA GPU detected; downloading and verifying a CUDA-enabled whisper.cpp build…"
                        .into()
                } else {
                    "Downloading and verifying whisper.cpp…".into()
                });
''',
    '''                progress(if prefer_cuda {
                    format!(
                        "NVIDIA GPU detected; downloading and verifying pinned whisper.cpp CUDA build {WHISPER_BUILD_TAG}…"
                    )
                } else {
                    format!(
                        "Downloading and verifying pinned whisper.cpp CPU build {WHISPER_BUILD_TAG}…"
                    )
                });
''',
)

old_install = '''fn install_whisper(tools_dir: &Path, temp_dir: &Path, prefer_cuda: bool) -> Result<(), String> {
    let zip_path = temp_dir.join("whisper.zip");
    let extract_dir = temp_dir.join("whisper");
    let target = tools_dir.join(executable_file_name("whisper-cli"));
    let script = format!(
        r#"$ErrorActionPreference='Stop';
[Net.ServicePointManager]::SecurityProtocol=[Net.SecurityProtocolType]::Tls12;
$headers=@{{'User-Agent'='StoryTeller-Lite'}};
$releases=Invoke-RestMethod -Headers $headers -Uri {api};
$assets=$releases | ForEach-Object {{ $_.assets }};
$asset=$null;
if ({prefer_cuda}) {{
  $asset=$assets | Where-Object {{ $_.name -match '(?i)^whisper-(cublas-.*-bin-x64|bin-win-cuda-.*x64)\\.zip$' }} | Sort-Object name -Descending | Select-Object -First 1;
}}
if (-not $asset) {{ $asset=$assets | Where-Object {{ $_.name -eq 'whisper-bin-x64.zip' }} | Select-Object -First 1; }}
if (-not $asset) {{ throw 'No official Windows x64 whisper.cpp binary asset was found.' }}
$digest=[string]$asset.digest;
if (-not $digest.StartsWith('sha256:')) {{ throw 'The whisper.cpp release asset did not publish a SHA-256 digest.' }}
Invoke-WebRequest -UseBasicParsing -Headers $headers -Uri $asset.browser_download_url -OutFile {zip};
$expected=$digest.Substring(7).ToUpperInvariant();
$actual=(Get-FileHash {zip} -Algorithm SHA256).Hash.ToUpperInvariant();
if ($actual -ne $expected) {{ throw 'whisper.cpp SHA-256 verification failed.' }}
Expand-Archive -LiteralPath {zip} -DestinationPath {extract} -Force;
$cli=Get-ChildItem -LiteralPath {extract} -Filter whisper-cli.exe -Recurse | Select-Object -First 1;
if (-not $cli) {{ $cli=Get-ChildItem -LiteralPath {extract} -Filter main.exe -Recurse | Select-Object -First 1; }}
if (-not $cli) {{ throw 'Neither whisper-cli.exe nor legacy main.exe was present in the downloaded archive.' }}
Get-ChildItem -LiteralPath $cli.Directory.FullName | ForEach-Object {{ Copy-Item -LiteralPath $_.FullName -Destination {tools} -Recurse -Force }};
if ($cli.Name -ieq 'main.exe') {{ Copy-Item -LiteralPath $cli.FullName -Destination {target} -Force; }}"#,
        api = ps_string(WHISPER_RELEASES_API),
        prefer_cuda = if prefer_cuda { "$true" } else { "$false" },
        zip = ps_path(&zip_path),
        extract = ps_path(&extract_dir),
        tools = ps_path(tools_dir),
        target = ps_path(&target),
    );
    run_powershell(&script)
}
'''
new_install = '''fn install_whisper(tools_dir: &Path, temp_dir: &Path, prefer_cuda: bool) -> Result<(), String> {
    let zip_path = temp_dir.join("whisper.zip");
    let extract_dir = temp_dir.join("whisper");
    let target = tools_dir.join(executable_file_name("whisper-cli"));
    let (asset_name, expected_sha256) = whisper_download_asset(prefer_cuda);
    let url = format!(
        "{WHISPER_DOWNLOAD_BASE}/{WHISPER_BUILD_TAG}/{asset_name}"
    );
    let script = format!(
        r#"$ErrorActionPreference='Stop';
[Net.ServicePointManager]::SecurityProtocol=[Net.SecurityProtocolType]::Tls12;
Invoke-WebRequest -UseBasicParsing -Uri {url} -OutFile {zip};
$expected={expected}.ToUpperInvariant();
$actual=(Get-FileHash {zip} -Algorithm SHA256).Hash.ToUpperInvariant();
if ($actual -ne $expected) {{ throw 'whisper.cpp SHA-256 verification failed.' }}
Expand-Archive -LiteralPath {zip} -DestinationPath {extract} -Force;
$cli=Get-ChildItem -LiteralPath {extract} -Filter whisper-cli.exe -Recurse | Select-Object -First 1;
if (-not $cli) {{ $cli=Get-ChildItem -LiteralPath {extract} -Filter main.exe -Recurse | Select-Object -First 1; }}
if (-not $cli) {{ throw 'Neither whisper-cli.exe nor legacy main.exe was present in the downloaded archive.' }}
Get-ChildItem -LiteralPath $cli.Directory.FullName | ForEach-Object {{ Copy-Item -LiteralPath $_.FullName -Destination {tools} -Recurse -Force }};
if ($cli.Name -ieq 'main.exe') {{ Copy-Item -LiteralPath $cli.FullName -Destination {target} -Force; }}"#,
        url = ps_string(&url),
        expected = ps_string(expected_sha256),
        zip = ps_path(&zip_path),
        extract = ps_path(&extract_dir),
        tools = ps_path(tools_dir),
        target = ps_path(&target),
    );
    run_powershell(&script)
}

fn whisper_download_asset(prefer_cuda: bool) -> (&'static str, &'static str) {
    if prefer_cuda {
        (WHISPER_CUDA_ASSET, WHISPER_CUDA_SHA256)
    } else {
        (WHISPER_CPU_ASSET, WHISPER_CPU_SHA256)
    }
}
'''
replace_exact(path, old_install, new_install)

replace_exact(
    path,
    '''    #[test]
    fn cuda_path_hint_is_recognized_without_dll_scan() {
        assert!(is_cuda_whisper_build(Path::new(
            "C:\\\\cache\\\\whisper-cpp-windows-x64-cuda-13.1.0\\\\whisper-cli.exe"
        )));
    }
''',
    '''    #[test]
    fn cuda_path_hint_is_recognized_without_dll_scan() {
        assert!(is_cuda_whisper_build(Path::new(
            "C:\\\\cache\\\\whisper-cpp-windows-x64-cuda-13.1.0\\\\whisper-cli.exe"
        )));
    }

    #[test]
    fn automatic_whisper_downloads_are_pinned_by_asset_and_digest() {
        assert_eq!(
            whisper_download_asset(false),
            (WHISPER_CPU_ASSET, WHISPER_CPU_SHA256)
        );
        assert_eq!(
            whisper_download_asset(true),
            (WHISPER_CUDA_ASSET, WHISPER_CUDA_SHA256)
        );
        assert_eq!(WHISPER_BUILD_TAG, "b5130");
        assert_eq!(WHISPER_CPU_SHA256.len(), 64);
        assert_eq!(WHISPER_CUDA_SHA256.len(), 64);
    }
''',
)

from pathlib import Path
import re


def sub_once(path, pattern, replacement, flags=re.S):
    p = Path(path)
    text = p.read_text(encoding="utf-8")
    text, count = re.subn(pattern, replacement, text, count=1, flags=flags)
    if count != 1:
        raise SystemExit(f"expected one match in {path}, found {count}: {pattern}")
    p.write_text(text, encoding="utf-8")


runtime = "crates/storyteller-ui/src/runtime_setup.rs"
sub_once(
    runtime,
    r'const FFMPEG_ZIP_URL: &str = "https://www\.gyan\.dev/ffmpeg/builds/ffmpeg-release-essentials\.zip";\nconst FFMPEG_SHA_URL: &str =\n    "https://www\.gyan\.dev/ffmpeg/builds/ffmpeg-release-essentials\.zip\.sha256";',
    '''const FFMPEG_VERSION: &str = "9.0.1";
const FFMPEG_ZIP_URL: &str =
    "https://github.com/GyanD/codexffmpeg/releases/download/9.0.1/ffmpeg-9.0.1-essentials_build.zip";
const FFMPEG_SHA256: &str = "fec81ae03971d9dd4be3ebe02e263bd2ec1d789483f931bdba5f5715e65da2e9";''',
)
sub_once(
    runtime,
    r'progress\("Downloading and verifying ffmpeg…"\.into\(\)\);',
    'progress(format!("Downloading and verifying pinned ffmpeg {FFMPEG_VERSION}…"));',
)
sub_once(
    runtime,
    r"fn install_ffmpeg\(tools_dir: &Path, temp_dir: &Path\) -> Result<\(\), String> \{.*?\n\}\n\nfn install_whisper",
    '''fn install_ffmpeg(tools_dir: &Path, temp_dir: &Path) -> Result<(), String> {
    let zip_path = temp_dir.join("ffmpeg.zip");
    let extract_dir = temp_dir.join("ffmpeg");
    let target = tools_dir.join(executable_file_name("ffmpeg"));
    let script = format!(
        r#"$ErrorActionPreference='Stop';
[Net.ServicePointManager]::SecurityProtocol=[Net.SecurityProtocolType]::Tls12;
Invoke-WebRequest -UseBasicParsing -Uri {url} -OutFile {zip};
$expected={expected}.ToUpperInvariant();
$actual=(Get-FileHash {zip} -Algorithm SHA256).Hash.ToUpperInvariant();
if ($actual -ne $expected) {{ throw 'ffmpeg SHA-256 verification failed.' }}
Expand-Archive -LiteralPath {zip} -DestinationPath {extract} -Force;
$exe=Get-ChildItem -LiteralPath {extract} -Filter ffmpeg.exe -Recurse | Select-Object -First 1;
if (-not $exe) {{ throw 'ffmpeg.exe was not present in the downloaded archive.' }}
Copy-Item -LiteralPath $exe.FullName -Destination {target} -Force;"#,
        url = ps_string(FFMPEG_ZIP_URL),
        expected = ps_string(FFMPEG_SHA256),
        zip = ps_path(&zip_path),
        extract = ps_path(&extract_dir),
        target = ps_path(&target),
    );
    run_powershell(&script)
}

fn install_whisper''',
)
sub_once(
    runtime,
    r"    #\[test\]\n    fn automatic_whisper_downloads_are_pinned_by_asset_and_digest\(\) \{",
    '''    #[test]
    fn automatic_ffmpeg_download_is_pinned_by_version_and_digest() {
        assert_eq!(FFMPEG_VERSION, "9.0.1");
        assert!(FFMPEG_ZIP_URL.contains("/9.0.1/ffmpeg-9.0.1-essentials_build.zip"));
        assert_eq!(FFMPEG_SHA256.len(), 64);
        assert_eq!(
            FFMPEG_SHA256,
            "fec81ae03971d9dd4be3ebe02e263bd2ec1d789483f931bdba5f5715e65da2e9"
        );
    }

    #[test]
    fn automatic_whisper_downloads_are_pinned_by_asset_and_digest() {''',
)

docs = "docs/RUNTIME.md"
sub_once(
    docs,
    r"- ffmpeg: Gyan Windows Essentials ZIP plus the provider's published `\.sha256`; the archive hash must match before extraction\.\n",
    "- ffmpeg: pinned Gyan/Codex FFmpeg `9.0.1` Windows Essentials ZIP (`ffmpeg-9.0.1-essentials_build.zip`), built from FFmpeg source commit `bf1b838f2a`; the downloaded archive must match SHA-256 `fec81ae03971d9dd4be3ebe02e263bd2ec1d789483f931bdba5f5715e65da2e9` before extraction. Lite does not follow the moving `ffmpeg-release-essentials.zip` URL during automatic install.\n",
)
print("ffmpeg pin patch applied")

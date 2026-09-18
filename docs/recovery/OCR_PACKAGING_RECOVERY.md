# OCR packaging recovery — Storyteller One-Click v0.39.0

This note records the OCR runtime/model stack physically bundled in the user-provided v0.39.0 installer. It is packaging/behavioral evidence for Lite's planned **lazy OCR**, not a requirement to carry the same Python stack forward.

## Recovered bundled stack

The old `sigil-repair` helper was frozen as a Python 3.12 Windows application and included a broad native/scientific runtime.

Confirmed OCR-related package versions from bundled metadata/binaries:

- **RapidOCR 3.9.2**
- **ONNX Runtime 1.28.0**
- **tesserocr 2.10.0**
- **Tesseract 5.5.2**

The frozen helper also contains supporting packages such as OpenCV, NumPy, Pillow, lxml, Shapely and their native DLLs.

This helps explain why the historical installer carried a large helper payload even though OCR was only relevant to a narrow unmatched-audio/Graphic Readout workflow.

## Bundled RapidOCR models

The NSIS file table and RapidOCR package RECORD identify these exact assets:

| Asset | Size |
| --- | ---: |
| `rapidocr/models/PP-OCRv6_det_small.onnx` | 9,929,594 bytes |
| `rapidocr/models/PP-OCRv6_rec_small.onnx` | 21,234,383 bytes |
| `rapidocr/models/ch_ppocr_mobile_v2.0_cls_mobile.onnx` | 585,532 bytes |

The package RECORD hashes also match the extracted NSIS data blocks.

The two PP-OCRv6 models alone are roughly 30 MiB uncompressed before ONNX Runtime/OpenCV/Python dependencies are counted.

## Tesseract fallback

The frozen helper contains:

- `tesserocr/tesseract55.dll`
- `tessdata/eng.traineddata`

The Tesseract DLL reports version **5.5.2**.

The English traineddata payload is approximately **4.11 MB** in the recovered archive.

The old helper therefore shipped an English-only Tesseract fallback rather than a full multilingual tessdata collection.

## Historical OCR flow

Recovered helper behavior used this stack lazily:

1. narrow the EPUB to bounded candidate documents/images;
2. extract embedded `alt`, `title`, and SVG text hints first;
3. run RapidOCR on candidate raster images that still need text;
4. fall back to English Tesseract for images with no accepted RapidOCR result;
5. use OCR/text evidence for Graphic Readout matching or supplemental-page transcript correction/fallback.

OCR was not conceptually required for ordinary audiobook transcription/alignment.

See `UNMATCHED_AUDIO_RECOVERY.md` and `ALLOCATOR_CANDIDATE_RECOVERY.md` for the classifier/candidate behavior.

## Lite packaging implication

The recovered Lite plan intentionally removed the **permanent OCR toggle** while retaining **lazy OCR**.

Do not interpret that as a requirement to permanently bundle the old frozen Python stack.

Preferred product/architecture goals are:

- OCR should initialize/run only when Smart classification or the current review segment actually needs image text;
- ordinary books that never need Graphic Readout/image-text analysis should pay minimal runtime cost;
- keep OCR implementation behind a narrow Rust-facing interface so the engine can change without changing allocator semantics;
- prefer embedded XHTML/SVG text hints before OCR;
- bound candidate images before invoking the OCR engine;
- keep OCR/model/runtime configuration out of the normal Lite Settings surface unless a real support requirement emerges.

Possible packaging strategies to benchmark later include:

1. a smaller native/in-process OCR dependency;
2. an optional/download-on-demand OCR component;
3. a compact bundled model/runtime if installer-size and startup-memory budgets are acceptable.

This recovery note deliberately does **not** choose among those options. The key finding is that reproducing the old frozen Python OCR environment would restore a large implementation dependency that Lite's refactor was intended to simplify.

## Licensing caution

This note only records package identities/versions/model filenames and behavior recovered from the user's installer. Any future OCR engine/model choice must independently review its applicable software/model licenses before distribution.

use thiserror::Error;

/// Crate-wide result type using `RustmotionError`.
pub type Result<T> = std::result::Result<T, RustmotionError>;

#[derive(Debug, Error)]
pub enum RustmotionError {
    #[error("{0}")]
    Generic(String),

    // --- IO / File errors ---
    #[error("Failed to read '{path}': {source}")]
    FileRead {
        path: String,
        source: std::io::Error,
    },

    // --- JSON parsing ---
    #[error("Failed to parse JSON: {source}")]
    JsonParse {
        #[from]
        source: serde_json::Error,
    },

    // --- HTML transpile ---
    #[error("HTML transpile error: {0}")]
    HtmlParse(String),

    // --- Asset loading ---
    #[error("Failed to load image '{path}': {reason}")]
    ImageLoad { path: String, reason: String },

    #[error("Failed to decode image '{path}'")]
    ImageDecode { path: String },

    #[error("SVG component must have either 'src' or 'data'")]
    SvgMissingSrc,

    #[error("Failed to load SVG '{path}': {reason}")]
    SvgLoad { path: String, reason: String },

    #[error("Failed to parse SVG: {reason}")]
    SvgParse { reason: String },

    #[error("Failed to create pixmap for {target}")]
    PixmapCreation { target: String },

    #[error("Invalid icon format: '{icon}' (expected 'prefix:name')")]
    InvalidIconFormat { icon: String },

    #[error("Failed to fetch icon '{icon}': {reason}")]
    IconFetch { icon: String, reason: String },

    #[error("Failed to parse icon SVG '{icon}': {reason}")]
    IconParse { icon: String, reason: String },

    #[error("Failed to create Skia image from {target}")]
    SkiaImageCreation { target: String },

    #[error("Failed to open GIF '{path}': {reason}")]
    GifOpen { path: String, reason: String },

    #[error("Failed to decode GIF '{path}': {reason}")]
    GifDecode { path: String, reason: String },

    #[error("QR code generation failed: {reason}")]
    QrCodeGeneration { reason: String },

    #[error("No fonts available on this system")]
    FontNotFound,

    // --- Include system ---
    #[error("Include depth limit ({limit}) exceeded while resolving '{path}'")]
    IncludeDepthExceeded { limit: u8, path: String },

    #[error("Include: scenes[{index}] is out of bounds in '{path}' (file has {total} scenes)")]
    IncludeSceneOutOfBounds {
        index: usize,
        path: String,
        total: usize,
    },

    #[error("Include: cannot resolve relative path '{path}' from inline JSON (use a file path or URL instead)")]
    IncludeInlinePath { path: String },

    #[error("Include: failed to fetch '{url}': {reason}")]
    IncludeRemoteFetch { url: String, reason: String },

    #[error("Include: file not found '{path}'")]
    IncludeFileNotFound { path: String },

    #[error("Scenario cannot have both top-level 'scenes' and 'composition' — use one or the other")]
    CompositionAndScenesConflict,

    #[error("Unknown background template '{name}' referenced via $ref")]
    UnknownBackgroundTemplate { name: String },

    #[error("Unknown heropattern '{name}' — see heropatterns.com for available patterns")]
    UnknownHeropattern { name: String },

    // --- Variables ---
    #[error("Variable '${name}' is not defined in '{path}'")]
    UndefinedVariable { name: String, path: String },

    #[error("Variable '{name}' in '{path}' is missing a default value")]
    VariableMissingDefault { name: String, path: String },

    #[error("Unresolved variable reference '${name}' after substitution in '{path}'")]
    UnresolvedVariable { name: String, path: String },

    #[error("Cannot interpolate non-string variable '${name}' into string in '{path}'")]
    VariableInterpolationTypeError { name: String, path: String },

    // --- Encoding ---
    #[error("No frames to render (total duration is 0)")]
    NoFrames,

    #[error("Failed to run ffmpeg: {reason}. Is ffmpeg installed?")]
    FfmpegSpawn { reason: String },

    #[error("FFmpeg encoding failed{}", .stderr.as_ref().map(|s| format!(": {}", s)).unwrap_or_default())]
    FfmpegFailed { stderr: Option<String> },

    #[error("Failed to open FFmpeg stdin pipe")]
    FfmpegPipe,

    #[error("Failed to write to FFmpeg pipe: {reason}")]
    FfmpegWrite { reason: String },

    #[error("Failed to wait for FFmpeg: {reason}")]
    FfmpegWait { reason: String },

    #[error("ffmpeg failed to extract frame from '{src}'")]
    FfmpegFrameExtract { src: String },

    #[error("Failed to create GIF encoder: {reason}")]
    GifEncoder { reason: String },

    #[error("Failed to set GIF repeat: {reason}")]
    GifRepeat { reason: String },

    #[error("Failed to write GIF frame: {reason}")]
    GifFrame { reason: String },

    // --- Audio ---
    #[error("Failed to open audio file '{path}': {reason}")]
    AudioOpen { path: String, reason: String },

    #[error("Failed to probe audio format for '{path}': {reason}")]
    AudioProbe { path: String, reason: String },

    #[error("No audio track found in '{path}'")]
    AudioNoTrack { path: String },

    #[error("Failed to create decoder for '{path}': {reason}")]
    AudioDecoder { path: String, reason: String },

    // --- Rendering ---
    #[error("Failed to create Skia surface")]
    SurfaceCreation,

    #[error("Failed to create image from pixels")]
    PixelImage,

    #[error("Failed to read pixels from Skia surface")]
    PixelRead,

    #[error("Failed to create motion blur surface")]
    MotionBlurSurface,

    // --- CLI ---
    #[error("Cannot use both input file and --json")]
    ConflictingInput,

    #[error("Provide either an input file or --json")]
    MissingInput,

    #[error("--watch requires an input file path (cannot use --json or stdin)")]
    WatchRequiresFile,

    #[error("Frame {frame} is out of range (total frames: {total})")]
    FrameOutOfRange { frame: u32, total: u32 },

    #[error("Time {time:.2}s is beyond video duration")]
    TimeOutOfRange { time: f64 },

    #[error("File watcher channel closed")]
    WatcherClosed,

    #[error("Validation failed: {schema_errors} schema error(s), {geometry_violations} geometry violation(s), {unresolved_vars} unresolved variable(s). Run `rustmotion validate -f <file>` to see details.")]
    ValidationFailed {
        schema_errors: usize,
        geometry_violations: usize,
        unresolved_vars: usize,
    },

    #[error("Incremental encoding unsupported: {reason}")]
    IncrementalUnsupported { reason: String },

    #[error("Invalid CRF value {value}: must be between 0 and 51")]
    InvalidCrf { value: u8 },

    #[error("Unknown codec '{codec}'. Supported: h264, h265, vp9, prores")]
    UnknownCodec { codec: String },

    #[error("Path is not valid UTF-8: '{path}'")]
    NonUtf8Path { path: String },

    // --- Preview ---
    #[error("Failed to create preview window: {reason}")]
    PreviewWindow { reason: String },

    // --- Lottie ---
    #[error("Failed to read Lottie file '{path}': {reason}")]
    LottieRead { path: String, reason: String },

    #[error("Lottie component requires either 'src' or 'data'")]
    LottieMissingSrc,

    #[error("Failed to read Lottie frame '{path}': {reason}")]
    LottieFrameRead { path: String, reason: String },

    #[error("Failed to decode Lottie frame '{path}': {reason}")]
    LottieFrameDecode { path: String, reason: String },

    #[error("Lottie render failed: {reason}")]
    LottieRender { reason: String },

    // --- Skills ---
    #[error("Unknown skill or rule: '{name}'. Run `rustmotion skills list` to see available rules.")]
    UnknownSkill { name: String },

    // --- IO ---
    #[error("{0}")]
    Io(#[from] std::io::Error),

    // --- External library errors ---
    #[error("Image processing error: {0}")]
    Image(#[from] image::ImageError),

    #[error("File watcher error: {0}")]
    Notify(#[from] notify::Error),

    #[error("{0}")]
    Other(String),
}

impl From<String> for RustmotionError {
    fn from(s: String) -> Self {
        RustmotionError::Other(s)
    }
}

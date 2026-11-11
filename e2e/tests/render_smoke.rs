//! Platform smoke test: renders a short video with the CPU backend and decodes it back
//! with the fframes decoder. Unlike the visual regression test it needs no reference
//! frames or external tools (ffmpeg CLI, odiff), so it runs on every OS in CI, Windows
//! included.

use fframes::ffmpeg_sys_fframes::{AVMediaType, avformat_close_input, avformat_open_input};
use fframes::media::FFmpegDecoder;
use fframes::{
    AudioMap, Color, Duration, FFramesContext, Frame, RenderOptions, Scene, Scenes, Svgr, Video,
};

const FPS: usize = 10;
const SWITCH_AT: usize = 10;

/// A solid color scene without audio.
#[derive(Debug)]
struct Fill(&'static str);

impl Scene for Fill {
    fn duration(&self) -> Duration<'_> {
        Duration::Frames(SWITCH_AT)
    }

    fn render_frame(&self, _: Frame, _: &FFramesContext) -> Svgr<'_> {
        fframes::svgr!(
            <svg xmlns="http://www.w3.org/2000/svg" width={SmokeVideo::WIDTH} height={SmokeVideo::HEIGHT}>
                <rect x="0" y="0" width={SmokeVideo::WIDTH} height={SmokeVideo::HEIGHT} fill={self.0} />
            </svg>
        )
    }
}

const RED: Fill = Fill("#ff0000");
const BLUE: Fill = Fill("#0000ff");

#[derive(Debug)]
struct SmokeVideo;

impl Video for SmokeVideo {
    const FPS: usize = FPS;
    const WIDTH: usize = 320;
    const HEIGHT: usize = 240;
    const BACKGROUND_COLOR: Color = Color::BLACK;

    fn duration(&self) -> Duration<'_> {
        Duration::Auto
    }

    fn audio(&self) -> AudioMap<'_> {
        AudioMap::none()
    }

    fn define_scenes(&self) -> Scenes<'_> {
        Scenes::from(vec![&RED as &dyn Scene, &BLUE as &dyn Scene])
    }

    fn render_frame<'a>(&'a self, frame: Frame, ctx: &FFramesContext<'a, '_>) -> Svgr<'a> {
        ctx.render_scenes(&frame)
    }
}

fn center_pixel(decoder: &mut FFmpegDecoder, frame: usize) -> [u8; 4] {
    unsafe {
        assert!(
            decoder.decode_up_to(frame as i64).unwrap(),
            "frame {frame} is missing from the rendered video"
        );

        let image = decoder
            .get_raw_frame()
            .convert_last_decoded_frame_into_svg_image(None)
            .unwrap();

        assert_eq!(image.width as usize, SmokeVideo::WIDTH);
        assert_eq!(image.height as usize, SmokeVideo::HEIGHT);

        let offset =
            ((image.height as usize / 2) * image.width as usize + image.width as usize / 2) * 4;
        image.data[offset..offset + 4].try_into().unwrap()
    }
}

fn assert_color_close(actual: [u8; 4], expected: [u8; 3], frame: usize) {
    // lossy yuv420p encoding shifts solid colors by a few units
    let close = actual[..3]
        .iter()
        .zip(expected)
        .all(|(a, e)| a.abs_diff(e) <= 24);

    assert!(
        close,
        "frame {frame}: expected ~{expected:?}, decoded {actual:?}"
    );
}

fn stream_types(path: &std::path::Path) -> Vec<AVMediaType> {
    let filename = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
    unsafe {
        let mut input = std::ptr::null_mut();
        assert_eq!(
            avformat_open_input(
                &raw mut input,
                filename.as_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            ),
            0
        );
        let types = (0..(*input).nb_streams as usize)
            .map(|index| (*(**(*input).streams.add(index)).codecpar).codec_type)
            .collect();
        avformat_close_input(&raw mut input);
        types
    }
}

#[test]
fn renders_and_decodes_video() {
    let dir = std::env::temp_dir().join(format!("fframes-smoke-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let output = dir.join("smoke.mp4");

    fframes::render(
        &output,
        &SmokeVideo,
        fframes::cpu::CpuRenderingBackend {
            concurrency: 2,
            ..Default::default()
        },
        &RenderOptions {
            logger: fframes::fframes_logger::FFramesLoggerVariant::Silent,
            tmp_files_directory: Some(&dir.join("chunks")),
            ..Default::default()
        },
    )
    .unwrap();

    assert!(std::fs::metadata(&output).unwrap().len() > 0);
    // A video without audio tracks must not get a silent audio stream.
    assert_eq!(stream_types(&output), [AVMediaType::AVMEDIA_TYPE_VIDEO]);

    let mut decoder = unsafe { FFmpegDecoder::new(&output, FPS, 1) }.unwrap();
    assert_color_close(center_pixel(&mut decoder, 0), [255, 0, 0], 0);
    assert_color_close(
        center_pixel(&mut decoder, SWITCH_AT + 2),
        [0, 0, 255],
        SWITCH_AT + 2,
    );
    drop(decoder);

    let _ = std::fs::remove_dir_all(&dir);
}

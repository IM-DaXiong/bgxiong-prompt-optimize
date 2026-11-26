//! 模块：video_prompt_adapters::h3::mode
//! 作用：MiniMax H3 五种官方模式与对齐头（VS30，自 `vendor_optimize/h3_prompt_mode.rs` 迁入）。
//!
//! - 模式只由冻结目标快照的中立 inputMode 决定：text_to_video → T2VA、image_to_video → I2VA、
//!   first_last_frame → FL2VA、last_frame_only → L2VA、multi_image_to_video / reference_to_video
//!   → Ref2VA。闭集外的模式显式拒绝，不塌缩成 T2VA。
//! - 画面角色取冻结参考清单的类型字段（`media_kind` first_frame / last_frame / reference_image，
//!   接上镜看 `source_kind` / `binding_id`），时间锚取冻结目标的 `durationSeconds`；不搜索原稿文字。
//! - FL2VA：Picture 1 = 首帧对 0.00 秒，Picture 2 = 尾帧对总时长；L2VA：唯一的 Picture 1 = 尾帧，
//!   对总时长（终点，不是起点）。首尾顺序按角色定，与清单顺序无关。缺时长显式拒绝，不用 0 秒默认。
//! - I2VA 首行是官方锁定句（Picture 1 在 0.00 秒完全参考）；Ref2VA 与 T2VA 没有对齐头，Ref2VA 的
//!   关键帧角色写在六段字段里；Ref2VA 的接上镜必须是 Picture 1。

use bgx_video_prompt_contract::inventory::ReferenceMediaInventoryItem;

/// Neutral input modes the H3 projector compiles (runtime-caps mode ids).
pub const H3_INPUT_MODES: [&str; 6] = [
    "text_to_video",
    "image_to_video",
    "first_last_frame",
    "last_frame_only",
    "multi_image_to_video",
    "reference_to_video",
];

const MEDIA_FIRST_FRAME: &str = "first_frame";
const MEDIA_LAST_FRAME: &str = "last_frame";
const MEDIA_IMAGE: &str = "reference_image";
const CONTINUITY_SOURCE_KIND: &str = "continuity_last_frame";
const CONTINUITY_BINDING_ID: &str = "continuity:start";

/// Official I2VA first line: Picture 1 locks the 0.00-second mark.
pub const I2VA_LOCK_LINE: &str =
    "For the target video, at 0.00 seconds into the target video, <Picture 1> (from [Shot 1]) is fully referenced.";

/// The five official H3 prompt modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum H3Mode {
    T2VA,
    I2VA,
    FL2VA,
    L2VA,
    Ref2VA,
}

impl H3Mode {
    /// Mode of one neutral input mode id; `None` outside the H3 closed set.
    pub fn from_input_mode(input_mode: &str) -> Option<Self> {
        match input_mode {
            "text_to_video" => Some(Self::T2VA),
            "image_to_video" => Some(Self::I2VA),
            "first_last_frame" => Some(Self::FL2VA),
            "last_frame_only" => Some(Self::L2VA),
            "multi_image_to_video" | "reference_to_video" => Some(Self::Ref2VA),
            _ => None,
        }
    }

    /// Official field that carries the picture description: Ref2VA uses the six-field layout.
    pub fn picture_field_key(self) -> &'static str {
        super::compile::picture_field_key(self == Self::Ref2VA)
    }

    /// FL2VA / L2VA anchor a picture at the end of the target video, so they need its duration.
    pub fn requires_duration_header(self) -> bool {
        matches!(self, Self::FL2VA | Self::L2VA)
    }
}

/// Typed role of one picture of the request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum H3PictureRole {
    /// Strict first frame (I2VA / FL2VA start).
    FirstFrame,
    /// Strict last frame (FL2VA / L2VA end).
    LastFrame,
    /// Reference picture (Ref2VA).
    Reference,
}

impl H3PictureRole {
    /// Role of a frozen reference row from its media kind; `None` for video / audio rows.
    pub fn of_media_kind(media_kind: &str) -> Option<Self> {
        match media_kind {
            MEDIA_FIRST_FRAME => Some(Self::FirstFrame),
            MEDIA_LAST_FRAME => Some(Self::LastFrame),
            MEDIA_IMAGE => Some(Self::Reference),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::FirstFrame => MEDIA_FIRST_FRAME,
            Self::LastFrame => MEDIA_LAST_FRAME,
            Self::Reference => MEDIA_IMAGE,
        }
    }
}

/// One picture of the request with its official number (`Picture N`).
#[derive(Debug, Clone, PartialEq)]
pub struct H3Picture {
    pub number: u32,
    pub role: H3PictureRole,
    pub reference_use_id: String,
}

/// Picture `picture` aligns with `seconds` into the target video.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct H3TimeAnchor {
    pub picture: u32,
    pub seconds: f64,
}

/// Mode, numbered pictures and time anchors of one H3 compile, all from frozen typed input.
#[derive(Debug, Clone, PartialEq)]
pub struct H3ModePlan {
    pub mode: H3Mode,
    pub pictures: Vec<H3Picture>,
    pub anchors: Vec<H3TimeAnchor>,
    /// Frozen target duration (validated `> 0` when the mode needs it).
    pub duration_seconds: Option<f64>,
}

/// Why the frozen input cannot form an H3 mode plan. Each one is an explicit refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum H3ModeError {
    /// Input mode outside the H3 closed set.
    InputModeUnsupported { input_mode: String },
    /// FL2VA / L2VA without a positive frozen target duration.
    DurationRequired { input_mode: String },
    /// The mode needs exactly one picture of this role and the frozen input has none.
    PictureRoleMissing {
        input_mode: String,
        role: H3PictureRole,
    },
    /// A frozen reference this mode cannot carry (wrong role, duplicate role or ordinal, missing
    /// ordinal, or a match-cut still that is not Picture 1).
    ReferenceUnsupported { reference_use_id: String },
}

impl H3ModeError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InputModeUnsupported { .. } => "H3_MODE_INPUT_MODE_UNSUPPORTED",
            Self::DurationRequired { .. } => "H3_MODE_DURATION_REQUIRED",
            Self::PictureRoleMissing { .. } => "H3_MODE_PICTURE_ROLE_MISSING",
            Self::ReferenceUnsupported { .. } => "H3_MODE_REFERENCE_UNSUPPORTED",
        }
    }
}

impl std::fmt::Display for H3ModeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InputModeUnsupported { input_mode } | Self::DurationRequired { input_mode } => {
                write!(f, "{}: {input_mode}", self.code())
            }
            Self::PictureRoleMissing { input_mode, role } => {
                write!(f, "{}: {input_mode} needs {}", self.code(), role.as_str())
            }
            Self::ReferenceUnsupported { reference_use_id } => {
                write!(f, "{}: {reference_use_id}", self.code())
            }
        }
    }
}

fn is_continuity(item: &ReferenceMediaInventoryItem) -> bool {
    item.source_kind.as_deref() == Some(CONTINUITY_SOURCE_KIND)
        || item.binding_id.as_deref() == Some(CONTINUITY_BINDING_ID)
}

/// Build the mode plan from the frozen target (`input_mode`, `duration_seconds`) and the frozen
/// reference rows the target accepts (in `referenceUseIds` order). Video / audio rows are only
/// carried by Ref2VA and are not pictures; they are left to the compiler.
pub fn plan_mode(
    input_mode: &str,
    duration_seconds: Option<f64>,
    references: &[&ReferenceMediaInventoryItem],
) -> Result<H3ModePlan, H3ModeError> {
    let mode =
        H3Mode::from_input_mode(input_mode).ok_or_else(|| H3ModeError::InputModeUnsupported {
            input_mode: input_mode.to_string(),
        })?;
    let duration = duration_seconds.filter(|d| d.is_finite() && *d > 0.0);
    if mode.requires_duration_header() && duration.is_none() {
        return Err(H3ModeError::DurationRequired {
            input_mode: input_mode.to_string(),
        });
    }
    let unsupported = |item: &ReferenceMediaInventoryItem| H3ModeError::ReferenceUnsupported {
        reference_use_id: item.item_id.clone(),
    };
    let mut first: Option<&ReferenceMediaInventoryItem> = None;
    let mut last: Option<&ReferenceMediaInventoryItem> = None;
    let mut pictures: Vec<H3Picture> = Vec::new();
    for &item in references {
        let role = H3PictureRole::of_media_kind(item.media_kind.as_str());
        match (mode, role) {
            (H3Mode::I2VA | H3Mode::FL2VA, Some(H3PictureRole::FirstFrame)) => {
                if first.replace(item).is_some() {
                    return Err(unsupported(item));
                }
            }
            (H3Mode::FL2VA | H3Mode::L2VA, Some(H3PictureRole::LastFrame)) => {
                if last.replace(item).is_some() {
                    return Err(unsupported(item));
                }
            }
            (H3Mode::Ref2VA, Some(H3PictureRole::Reference)) => {
                let number = item.ordinal_within_kind;
                if number == 0 || pictures.iter().any(|p| p.number == number) {
                    return Err(unsupported(item));
                }
                if is_continuity(item) && number != 1 {
                    return Err(unsupported(item));
                }
                pictures.push(H3Picture {
                    number,
                    role: H3PictureRole::Reference,
                    reference_use_id: item.item_id.clone(),
                });
            }
            (H3Mode::Ref2VA, None) => {}
            _ => return Err(unsupported(item)),
        }
    }
    let missing = |role: H3PictureRole| H3ModeError::PictureRoleMissing {
        input_mode: input_mode.to_string(),
        role,
    };
    let picture =
        |number: u32, role: H3PictureRole, item: &ReferenceMediaInventoryItem| H3Picture {
            number,
            role,
            reference_use_id: item.item_id.clone(),
        };
    let end = duration.unwrap_or_default();
    let (pictures, anchors) = match mode {
        H3Mode::T2VA => (Vec::new(), Vec::new()),
        H3Mode::I2VA => {
            let f = first.ok_or_else(|| missing(H3PictureRole::FirstFrame))?;
            (
                vec![picture(1, H3PictureRole::FirstFrame, f)],
                vec![H3TimeAnchor {
                    picture: 1,
                    seconds: 0.0,
                }],
            )
        }
        H3Mode::FL2VA => {
            let f = first.ok_or_else(|| missing(H3PictureRole::FirstFrame))?;
            let l = last.ok_or_else(|| missing(H3PictureRole::LastFrame))?;
            (
                vec![
                    picture(1, H3PictureRole::FirstFrame, f),
                    picture(2, H3PictureRole::LastFrame, l),
                ],
                vec![
                    H3TimeAnchor {
                        picture: 1,
                        seconds: 0.0,
                    },
                    H3TimeAnchor {
                        picture: 2,
                        seconds: end,
                    },
                ],
            )
        }
        H3Mode::L2VA => {
            let l = last.ok_or_else(|| missing(H3PictureRole::LastFrame))?;
            (
                vec![picture(1, H3PictureRole::LastFrame, l)],
                vec![H3TimeAnchor {
                    picture: 1,
                    seconds: end,
                }],
            )
        }
        H3Mode::Ref2VA => {
            let mut pictures = pictures;
            pictures.sort_by_key(|p| p.number);
            (pictures, Vec::new())
        }
    };
    Ok(H3ModePlan {
        mode,
        pictures,
        anchors,
        duration_seconds: duration,
    })
}

/// Official `S.SS` second mark.
pub fn format_duration_ss(duration_seconds: f64) -> String {
    format!("{:.2}", duration_seconds)
}

/// Official alignment header (with its trailing newline) of `mode` for a frozen duration.
/// T2VA / Ref2VA have none; FL2VA / L2VA without a positive duration are refused.
pub fn alignment_header(
    mode: H3Mode,
    duration_seconds: Option<f64>,
) -> Result<Option<String>, H3ModeError> {
    let end = || {
        duration_seconds
            .filter(|d| d.is_finite() && *d > 0.0)
            .map(format_duration_ss)
            .ok_or_else(|| H3ModeError::DurationRequired {
                input_mode: match mode {
                    H3Mode::FL2VA => "first_last_frame",
                    _ => "last_frame_only",
                }
                .to_string(),
            })
    };
    Ok(match mode {
        H3Mode::I2VA => Some(format!("{I2VA_LOCK_LINE}\n")),
        H3Mode::FL2VA => {
            let ss = end()?;
            Some(format!(
                "How the reference pictures align with the target video — Picture 1 (from Shot 1) aligns with the 0.00-second mark of the target video; Picture 2 (from Shot 1) aligns with the {ss}-second mark of the target video.\n"
            ))
        }
        H3Mode::L2VA => {
            let ss = end()?;
            Some(format!(
                "How the reference pictures align with the target video — <Picture 1> (from [Shot 1]) aligns with the {ss}-second mark of the target video.\n"
            ))
        }
        H3Mode::Ref2VA | H3Mode::T2VA => None,
    })
}

impl H3ModePlan {
    /// Official alignment header of this plan. The end mark is the latest time anchor (FL2VA /
    /// L2VA anchor their last picture at the duration validated by [`plan_mode`]).
    pub fn alignment_header(&self) -> Result<Option<String>, H3ModeError> {
        let end_mark = self
            .anchors
            .iter()
            .map(|a| a.seconds)
            .reduce(f64::max)
            .filter(|s| *s > 0.0);
        alignment_header(self.mode, end_mark.or(self.duration_seconds))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, kind: &str, ordinal: u32) -> ReferenceMediaInventoryItem {
        ReferenceMediaInventoryItem {
            token: String::new(),
            media_kind: kind.into(),
            label: "pad".into(),
            binding_id: None,
            source_kind: None,
            media_relative_path: None,
            audio_usage: None,
            item_id: id.into(),
            declared_entity_kind: String::new(),
            declared_identity: None,
            ordinal_within_kind: ordinal,
        }
    }

    #[test]
    fn modes_come_from_the_neutral_closed_set_only() {
        for (id, mode) in [
            ("text_to_video", H3Mode::T2VA),
            ("image_to_video", H3Mode::I2VA),
            ("first_last_frame", H3Mode::FL2VA),
            ("last_frame_only", H3Mode::L2VA),
            ("multi_image_to_video", H3Mode::Ref2VA),
            ("reference_to_video", H3Mode::Ref2VA),
        ] {
            assert_eq!(H3Mode::from_input_mode(id), Some(mode));
            assert!(H3_INPUT_MODES.contains(&id));
        }
        for id in ["camera_control", "video_edit", ""] {
            assert_eq!(H3Mode::from_input_mode(id), None);
            assert!(matches!(
                plan_mode(id, Some(6.0), &[]),
                Err(H3ModeError::InputModeUnsupported { .. })
            ));
        }
    }

    #[test]
    fn last_frame_only_picture_one_is_the_end() {
        let last = item("u-last", "last_frame", 1);
        let plan = plan_mode("last_frame_only", Some(6.0), &[&last]).unwrap();
        assert_eq!(plan.pictures.len(), 1);
        assert_eq!(plan.pictures[0].number, 1);
        assert_eq!(plan.pictures[0].role, H3PictureRole::LastFrame);
        assert_eq!(
            plan.anchors,
            vec![H3TimeAnchor {
                picture: 1,
                seconds: 6.0
            }]
        );
        let header = plan.alignment_header().unwrap().unwrap();
        assert!(header.contains("<Picture 1> (from [Shot 1]) aligns with the 6.00-second mark"));
        assert!(!header.contains("0.00-second"));
    }

    #[test]
    fn first_last_order_follows_roles_and_frozen_duration() {
        let first = item("u-first", "first_frame", 1);
        let last = item("u-last", "last_frame", 1);
        let plan = plan_mode("first_last_frame", Some(4.5), &[&last, &first]).unwrap();
        assert_eq!(plan.pictures[0].reference_use_id, "u-first");
        assert_eq!(plan.pictures[1].reference_use_id, "u-last");
        assert_eq!(plan.anchors[1].seconds, 4.5);
        let header = plan.alignment_header().unwrap().unwrap();
        assert!(header.contains("Picture 1 (from Shot 1) aligns with the 0.00-second mark"));
        assert!(header.contains("Picture 2 (from Shot 1) aligns with the 4.50-second mark"));
    }

    #[test]
    fn missing_duration_is_refused_never_zero() {
        let first = item("u-first", "first_frame", 1);
        let last = item("u-last", "last_frame", 1);
        for duration in [None, Some(0.0), Some(-1.0), Some(f64::NAN)] {
            assert!(matches!(
                plan_mode("first_last_frame", duration, &[&first, &last]),
                Err(H3ModeError::DurationRequired { .. })
            ));
            assert!(matches!(
                plan_mode("last_frame_only", duration, &[&last]),
                Err(H3ModeError::DurationRequired { .. })
            ));
            assert!(alignment_header(H3Mode::L2VA, duration).is_err());
            assert!(alignment_header(H3Mode::FL2VA, duration).is_err());
        }
    }

    #[test]
    fn frames_must_match_the_mode() {
        let first = item("u-first", "first_frame", 1);
        let last = item("u-last", "last_frame", 1);
        let image = item("u-img", "reference_image", 1);
        assert!(matches!(
            plan_mode("first_last_frame", Some(6.0), &[&first]),
            Err(H3ModeError::PictureRoleMissing {
                role: H3PictureRole::LastFrame,
                ..
            })
        ));
        assert!(matches!(
            plan_mode("last_frame_only", Some(6.0), &[&first, &last]),
            Err(H3ModeError::ReferenceUnsupported { .. })
        ));
        assert!(matches!(
            plan_mode("image_to_video", None, &[&first, &image]),
            Err(H3ModeError::ReferenceUnsupported { .. })
        ));
        assert!(matches!(
            plan_mode("text_to_video", None, &[&image]),
            Err(H3ModeError::ReferenceUnsupported { .. })
        ));
        let i2va = plan_mode("image_to_video", None, &[&first]).unwrap();
        assert_eq!(
            i2va.alignment_header().unwrap().as_deref(),
            Some(format!("{I2VA_LOCK_LINE}\n").as_str())
        );
    }

    #[test]
    fn ref2va_numbers_pictures_by_frozen_ordinal_and_has_no_header() {
        let a = item("u-a", "reference_image", 2);
        let b = item("u-b", "reference_image", 1);
        let audio = item("u-audio", "reference_audio", 1);
        let plan = plan_mode("reference_to_video", None, &[&a, &audio, &b]).unwrap();
        let numbers: Vec<u32> = plan.pictures.iter().map(|p| p.number).collect();
        assert_eq!(numbers, vec![1, 2]);
        assert_eq!(plan.pictures[0].reference_use_id, "u-b");
        assert!(plan.anchors.is_empty());
        assert_eq!(plan.alignment_header().unwrap(), None);
        let unnumbered = item("u-x", "reference_image", 0);
        assert!(plan_mode("multi_image_to_video", None, &[&unnumbered]).is_err());
    }

    #[test]
    fn ref2va_match_cut_still_must_be_picture_one() {
        let mut cont = item("u-cont", "reference_image", 2);
        cont.source_kind = Some(CONTINUITY_SOURCE_KIND.into());
        let user = item("u-user", "reference_image", 1);
        assert!(matches!(
            plan_mode("reference_to_video", None, &[&user, &cont]),
            Err(H3ModeError::ReferenceUnsupported { reference_use_id }) if reference_use_id == "u-cont"
        ));
        let mut cont1 = item("u-cont", "reference_image", 1);
        cont1.binding_id = Some(CONTINUITY_BINDING_ID.into());
        let user2 = item("u-user", "reference_image", 2);
        assert!(plan_mode("reference_to_video", None, &[&user2, &cont1]).is_ok());
    }
}

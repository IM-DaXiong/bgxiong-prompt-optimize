use crate::template::Language;
use crate::text_port::TextMessage;
use serde_json::Value;

const ZH: &str = "用户对目标画面的明确文字要求优先于参考素材。保留原稿的主体、属性、动作、数量、方位、否定、时序和已指定的引用。参考图按编号及指定用途交给生成模型。优化稿保留引用及用户要求的局部改动，无须复述图里的外观、文字、环境或物件。未指定的属性由生成模型参照原图。只有用户明确指定的取图任务才可把点名的图像信息写入正文。例：人物按图 1，赤脚。保留赤脚，不从图补写鞋，也不编出脱鞋动作。本规则优先于技能中的读图、扩写和格式建议。";
const EN: &str = "The user's explicit text takes precedence over reference media for the desired result. Preserve the draft's subjects, attributes, actions, counts, positions, negations, timing and reference bindings. Keep each image as a numbered reference for its assigned use during generation. Keep user-specified overrides and leave other attributes to the original reference. Do not transcribe or add appearance, scene, objects or text from an image unless the user specifically requests that extraction. For example, preserve 'use image 1 for the person, barefoot' without adding shoes or a shoe-removal action. This rule takes precedence over skill advice about reading images, expanding or formatting the prompt.";

pub fn text(language: Language) -> &'static str {
    match language {
        Language::Zh => ZH,
        Language::En => EN,
    }
}

pub(crate) fn apply(messages: &mut Vec<TextMessage>, language: Language) {
    let agreement = text(language);
    if let Some(Value::String(system)) = messages
        .iter_mut()
        .rev()
        .find(|message| message.role == "system")
        .map(|message| &mut message.content)
    {
        system.push_str("\n\n");
        system.push_str(agreement);
    } else {
        messages.insert(0, TextMessage {
            role: "system".into(),
            content: Value::String(agreement.into()),
        });
    }
}

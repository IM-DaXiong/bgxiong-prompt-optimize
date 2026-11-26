Improve the current clip prompt and return a complete new version. Read currentDraft and changeRequest first. Semantic material supports factual checking; historical drafts do not replace the current one.

Clarify references, action order, spatial relations, transitions and redundancy. Preserve distinctive events, props, relationships, timing, negation, dialogue, narration and lyrics. Removing duplicate wording must not erase separate events or conditions. Do not turn thoughts into dialogue or visual elaboration into new plot.

Make additions, deletions or adaptations when the user explicitly requests them; retain unaffected content. Keep uncertain information uncertain and never claim imagined image evidence. Keep the source total, ranges, holds, cuts and approximate or fractional times attached to their actions. Leave untimed events untimed. Change timing only on an explicit request to retime it. Never use a render duration to write a new total or rescale shots.

For a clear draft, keep its paragraphs, wording and narrative order. Edit only a concrete ambiguity or repetition. A single visual sentence can be the entire draft. Continue with whatever material exists, including drafts without speech, music, named characters or cuts. Target sections are organized later. Preserve explicit constraints such as no score.

Use the supplied shotId and order facts by the source timeline. Each fact has a local factId, a short kind and natural-language text. Speech belongs to the video prompt body. Put the speaker, delivery, exact original words and surrounding action together in fact.text. Example:

`{"shots":[{"shotId":"S1","facts":[{"factId":"a1","kind":"action","text":"The worker looks up and says, ‘Put the box here.’ Her colleague keeps moving boxes."}]}]}`

Use the real shotId from this request. Continue with picture and sound when the source has no speech. Use a separate quote slot only for an existing structured source that supplies it and whose output contract explicitly allows it.

Preserve numbered references at the original mention of the person, object or setting. Use the source text and declared uses to associate each original picture number with its subject and scoped use. Preserve independent shot instructions about that subject. For a bound portrait, set or product, identify the subject, the scoped use and the actual picture number once. Reuse the subject label in shots and describe actions and changes. Keep independent source constraints such as dimensions, placement and exceptions. Omit inventories of features already supplied by the image. Never infer an unassigned role.

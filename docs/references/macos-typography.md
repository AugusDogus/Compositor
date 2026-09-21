# macOS text styles

The [Apple typography guidelines](https://developer.apple.com/design/human-interface-guidelines/typography), retrieved on 2026-09-20, specify these macOS styles:

| Style | Size | Line height | Weight |
| --- | --- | --- | --- |
| Headline | 13 pt | 16 pt | Bold |
| Body | 13 pt | 16 pt | Regular |
| Callout | 12 pt | 15 pt | Regular |
| Caption 1 | 10 pt | 13 pt | Regular |
| Title 2 | 17 pt | 22 pt | Regular |

The machine-readable source is Apple's [typography documentation JSON](https://developer.apple.com/tutorials/data/design/human-interface-guidelines/typography.json), in the macOS specifications table. Its latest listed revision is December 16, 2025.

The Swift source uses default body text for dialog labels and values, callouts for selection and size summaries, and captions for Curves instructions, Hue angle values, Levels field labels and histogram notes, and the color picker's sampling hint. Explicit tool-header fonts remain 12-point controls and 13-point semibold titles. This is a reference for text styles, not evidence of identical SF Pro and Inter rasterization.

Canvas Size, Image Size and JPEG export explicitly apply bold weight to Title 2. New Canvas applies semibold to its Title 2 heading and medium to its Callout dimension labels. Mask palette popovers use Headline. The Image Size current dimensions and pixels/inch unit are Body text, while its explanatory and result summaries are Callouts.

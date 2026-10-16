# 🎮 Reverse Controller (Controller Companion — Native Rust Edition)

## الجديد في v1.2.0 — Keyboard & Reliability

- **لوحة تحرير للكيبورد بالعربية والإنجليزية:** زر **Edit** يفتح النسخ واللصق والقص، التراجع والإعادة، تحديد الكل، حذف كلمة، والأسهم وHome/End. اضغط **ABC / حروف** للعودة.
- **كتابة عربية أفضل:** تحسين اقتراحات «لا» والتشكيل والترقيم، وإلغاء الاقتراحات القديمة عند تغيير النافذة أو تحريك مؤشر الكتابة عبر الكيبورد.
- **معايرة العصا:** قياس الانحراف خلال أربع ثوانٍ واقتراح مناطق الخمول، مع أنماط Precision وBalanced وLarge display.
- **موثوقية أعلى:** حفظ آمن، التحقق من الإعدادات، تحسين الاستعادة عند انقطاع اليد، وعزل جلسات التسجيل الصوتي.
- **اختبارات:** 126 اختبارًا للنسخة الكاملة و86 دون واجهة؛ اختبار المؤشر التفاعلي منفصل. تجربة الأجهزة الفعلية ما زالت مطلوبة.

**[تحميل آخر إصدار](https://github.com/m7kor/Reverse-Controller/releases/latest)** · **[ملاحظات v1.2.0](docs/releases/v1.2.0.md)**

> دليل الكيبورد الحالي: **A** لاختيار المفتاح، **X** للمسح، **Y** للمسافة، **LB/RB** لتحريك مؤشر الكتابة، **LT** للرموز، **RT** لـShift، **Back** لتبديل اللغة، و**R3** لتحريك نافذة الكيبورد. استخدم زر **?** لعرض المساعدة.

[![Rust](https://img.shields.io/badge/Rust-2021%20Edition-orange?logo=rust)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/Platform-Windows%2010%20%2F%2011-blue?logo=windows)](https://microsoft.com/windows)
[![Release](https://img.shields.io/github/v/release/m7kor/Reverse-Controller?color=blue&logo=github)](https://github.com/m7kor/Reverse-Controller/releases/latest)
[![License](https://img.shields.io/badge/License-MIT-green)](LICENSE)

A high-performance, ultra-lightweight native **Rust** re-engineering of the Windows desktop controller utility **Controller Companion**. It seamlessly transforms your Xbox/XInput controller into a mouse and keyboard with Windows 11 Fluent Acrylic visuals, full native Arabic keyboard support, and 100% offline AI voice dictation.

إعادة هندسة وتطوير جذرية لبرنامج **Controller Companion** بلغة **Rust** الصافية لسطح مكتب ويندوز؛ لتحويل يد التحكم (Xbox / XInput) إلى ماوس وكيبورد متكامل وسلس، مع واجهات شفافة حديثة (Fluent Acrylic)، ودعم أصيل وكامل للغة العربية، ومحرك إملاء صوتي بالذكاء الاصطناعي يعمل محلياً بالكامل (أوفلاين بدون إنترنت).

---

## ✨ Features (المميزات الرئيسية)

- ⚡ **Ultra-Lightweight & Efficient (استهلاك فائق الخفة للموارد):**
  - Background resource use depends on enabled features and the loaded voice model; measurements are pending.
  - Overlays reduce repainting while hidden.
  - Controller input is polled; response time depends on polling and system scheduling.
- 🎯 **Pixel-Perfect Stick Physics & AltSnap:**
  - Independent deadzones, non-linear acceleration curves, and sub-pixel fractional leftovers.
  - Diagonal 8-way snapping and 4-way vertical scroll locking.
  - Sniper precision mode (LT button) for fine precision control.
- 🔄 **Universal Hardware Back+Start Chord (اختصار التشغيل والإيقاف الفوري):**
  - Pressing **Back + Start** at any time instantly toggles the controller between desktop mouse mode and native game mode.
  - Works **even while disabled**, with haptic rumble pulse and animated OSD feedback.
- 🎮 **Automatic Game / Fullscreen Detection:**
  - Auto-pauses input when exclusive or borderless fullscreen games are focused, leaving full controller access to the game.
- ⌨️ **Windows 11 Fluent 2 Translucent Keyboard:**
  - Beautiful Mica / acrylic translucent dark theme with sound effects (mechanical switch SFX).
  - **Full Arabic Layout:** Native bidirectional shaping, ligatures (`لا`), and full Harakat/Tashkeel support (`َ ِ ُ ً ٍ ٌ ّ ْ`).
  - Automatic smart repositioning to avoid covering the active cursor/caret.
  - Smart auto-open when clicking text fields with the controller.
- 🎙️ **100% Offline AI Voice Dictation (إملاء صوتي بالذكاء الاصطناعي أوفلاين):**
  - Local speech-to-text powered by Whisper GGML (zero internet connection needed, 100% private).
  - Automatic Voice Activity Detection (VAD) stops recording upon speech pauses.
  - Automatic model unloading after 75s of idle to reclaim ~120 MB RAM instantly.
- 🍕 **Quick 8-Way Radial Pie Menu:**
  - Fast thumbstick selector for common actions (Browser, Task View, Explorer, Media, Virtual Desktops).
- 🛡️ **Guaranteed Fail-Safe Haptics (أمان الاهتزاز التام):**
  - Independent hardware timers stop vibration motors across all 4 slots automatically.
- ⚙️ **Modern Tray & Settings UI:**
  - Left-click tray icon to open Settings safely.
  - Right-click for full context menu with instant toggle switch (`✔ Enable Controller`).
  - Import existing `.amgp` profiles with automatic conversion.

---

## 🎮 الدليل الشامل لجميع الأزرار والاختصارات باليد (Full Controller Guide)

تمت هندسة توزيع الأزرار بدقة لتمكينك من التحكم الكامل بجهاز الكمبيوتر والتصفح والكتابة والألعاب دون لمس الكيبورد أو الماوس إطلاقاً.

---

### 1️⃣ الطبقة الأساسية للتحكم بالماوس والوسائط (Desktop Base Layer)

الوضع الافتراضي عند تشغيل البرنامج واستخدام اليد على سطح المكتب:

| الزر في اليد (Controller Input) | الوظيفة الأساسية (Action) | الشرح والتفاصيل (Function & Description) |
| :--- | :--- | :--- |
| **🕹️ عصا الأنالوج اليسرى (Left Stick)** | **تحريك مؤشر الماوس (Mouse Move)** | تحريك ناعم وفائق الدقة للماوس مع تسارع فيزيائي ذكي ومقاومة للارتجاف (Deadzone). |
| **🕹️ عصا الأنالوج اليمنى (Right Stick)** | **عجلة التمرير (Mouse Scroll)** | تمرير عمودي للأعلى والأسفل وتمرير أفقي لتصفح الصفحات والمستندات بسهولة. |
| **🟢 زر A** | **النقر بالزر الأيسر للماوس (Left Click)** | النقر الأساسي، السحب، وفتح الملفات والروابط. |
| **🔵 زر X** | **النقر بالزر الأيمن للماوس (Right Click)** | فتح القائمة الجانبية (Context Menu) وتخصيص العناصر. |
| **🔴 زر B** | **زر المسح والتراجع (Backspace)** | مسح الحرف السابق أثناء الكتابة أو الرجوع خطوة للخلف. |
| **🟡 زر Y** | **تشغيل / إيقاف الوسائط (Play / Pause)** | التحكم الفوري بتشغيل أو إيقاف الموسيقى والفيديوهات في ويندوز. |
| **🔘 ضغطة الأنالوج الأيمن (R3 - Right Thumb)** | **الزر الأوسط للماوس (Middle Click)** | إغلاق تبويبات المتصفح بنقرة واحدة، أو تفعيل التمرير التلقائي. |
| **🔘 ضغطة الأنالوج الأيسر (L3 - Left Thumb)** | **إظهار / إخفاء الكيبورد الشفاف (Toggle Keyboard)** | فتح أو إغلاق لوحة المفاتيح الافتراضية العائمة الفاخرة على الشاشة. |
| **🎯 الزناد الأيسر (LT - Left Trigger)** | **نمط القناص الدقيق + زر Ctrl (Sniper Mode & Ctrl)** | **خاصية مميزة:** عند الضغط عليه يبطئ حركة الماوس بنسبة 70% للتحكم المليمتري الدقيق، ويعمل كزر `Ctrl` للمتصفحات. |
| **⚡ الزناد الأيمن (RT - Right Trigger)** | **زر Shift (Shift Modifier)** | يعمل كزر `Shift` لتحديد النصوص أو الملفات المتعددة. |
| **⏮️ الكتف الأيسر (LB - Left Shoulder)** | **المقطع السابق (Previous Media Track)** | الانتقال للأغنية أو المقطع السابق. |
| **⏭️ الكتف الأيمن (RB - Right Shoulder)** | **المقطع التالي (Next Media Track)** | الانتقال للأغنية أو المقطع التالي. |
| **🎛️ أزرار الاتجاهات (D-Pad: ⬆️ ⬇️ ⬅️ ➡️)** | **مفاتيح الأسهم (Arrow Keys)** | التنقل بالأسهم بين العناصر والنصوص والصفحات مع تكرار مستمر عند الاستمرار بالضغط. |
| **▶️ زر Start** | **زر الإدخال (Enter)** | فتح البرامج، إرسال الرسائل، أو النزول لسطر جديد. |
| **📋 زر Back (View)** | **مفتاح الطبقة المتقدمة (Modifier Key)** | عند تعليق الضغط عليه، يحول باقي أزرار اليد إلى اختصارات ثانوية خارقة (انظر الجدول التالي). |
| **🌐 زر Xbox Guide (اللوجو الكبير)** | **قائمة ابدأ (Windows Start Menu)** | نقرة واحدة تفتح قائمة ابدأ لويندوز. (انظر ميزة الإطفاء الذاتي بالأسفل). |

---

### 2️⃣ طبقة الاختصارات المتقدمة (Back Modifier Layer: اضغط مع زر Back)

عند **الاستمرار بالضغط على زر Back**، تتحول أزرار اليد إلى مركز تحكم متقدم بالنظام:

| الاختصار (Button Combo) | الوظيفة (Action) | الشرح والتفاصيل (Function & Description) |
| :--- | :--- | :--- |
| **Back + Start** | **تشغيل / إيقاف البرنامج (Toggle Controller)** | **الاختصار الشامل:** تشغيل أو إيقاف تحكم اليد بسطح المكتب فوراً مع اهتزاز تأكيدي وإشعار OSD. |
| **Back + LB (الكتف الأيسر)** | **الإملاء الصوتي بالذكاء الاصطناعي (AI Voice Typing)** | يبدأ تسجيل صوتك مباشرة وتحويله لنص عربي أو إنجليزي عبر Whisper محلياً بدون إنترنت. |
| **Back + R3 (ضغطة الأنالوج الأيمن)** | **القائمة الدائرية السريعة (Radial Pie Menu)** | يفتح قائمة دائرية سريعة بـ 8 اتجاهات للتنقل السريع بالأنالوج لأهم البرامج. |
| **Back + L3 (ضغطة الأنالوج الأيسر)** | **قفل السحب والإفلات (Drag Lock)** | يقوم بتعليق زر الماوس الأيسر تلقائياً لتسحب النوافذ أو الملفات دون الحاجة للاستمرار بالضغط. |
| **Back + D-Pad ⬆️ (سهم فوق)** | **رفع مستوى الصوت (Volume Up)** | زيادة صوت النظام مع مؤشر مرئي سلس وتغذية صوتية. |
| **Back + D-Pad ⬇️ (سهم تحت)** | **خفض مستوى الصوت (Volume Down)** | خفض صوت النظام مع مؤشر مرئي سلس وتغذية صوتية. |
| **Back + D-Pad ⬅️ (سهم يسار)** | **محاذاة النافذة لليسار (Snap Window Left)** | تصغير النافذة النشطة ومحاذاتها للنصف الأيسر من الشاشة (Win+Left). |
| **Back + D-Pad ➡️ (سهم يمين)** | **محاذاة النافذة لليمين (Snap Window Right)** | تصغير النافذة النشطة ومحاذاتها للنصف الأيمن من الشاشة (Win+Right). |
| **Back + 🔴 B** | **مفتاح الهروب (Escape)** | إغلاق القوائم المفتوحة، النوافذ المنبثقة، أو الخروج من وضع ملء الشاشة. |
| **Back + 🔵 X** | **إظهار سطح المكتب (Show Desktop)** | تصغير جميع النوافذ المفتوحة فوراً وإظهار سطح المكتب (Win+D). |
| **Back + RB (الكتف الأيمن)** | **مبدل المهام (Task Switcher / Alt+Tab)** | تفعيل مبدل النوافذ ثلاثي الأبعاد للتنقل بين البرامج المفتوحة. |
| **Back + 🟢 A** | **شاشة العرض الخارجية (Display Switch - External)** | التبديل الفوري لمخرج الشاشة الخارجية / البروجكتر / التلفاز. |
| **Back + 🟡 Y** | **شاشة العرض الداخلية (Display Switch - Internal)** | العودة لشاشة اللابتوب أو الشاشة الرئيسية. |
| **Back + 🌐 Guide** | **إطفاء يد التحكم (Turn Off Pad)** | إيقاف تشغيل يد التحكم اللاسلكية فوراً لحفظ البطارية. |

---

### 3️⃣ اختصارات العتاد والنظام الحصرية (Hardware & System Universal Chords)

* 🔄 **التشغيل والإيقاف الشامل (Universal Toggle Chord: Back + Start):**
  - بالضغط على زر **Back + Start** معاً، يمكنك إيقاف البرنامج أو تشغيله في أي ثانية.
  - **الميزة الحصرية:** هذا الاختصار يعمل على مستوى العتاد دائماً **حتى لو كان البرنامج معطلاً**، مما يتيح لك استعادة التحكم بالماوس فوراً دون لمس الكيبورد.
* 🔋 **إطفاء يد التحكم اللاسلكية بلمسة واحدة (Guide Hold-to-Power-Off):**
  - قم بتعليق الضغط على **زر Xbox Guide (الشعار)** لمدة **2.5 ثانية**.
  - ستقوم اليد بإطلاق نبضة اهتزاز مزدوجة ثم إطفاء اليد فوراً لحفظ طاقة البطارية دون الحاجة لنزع البطاريات.
* 🎯 **نمط القناص والتصويب الفائق (Sniper Aiming):**
  - عند الاستمرار بالضغط على **LT**، يتم تقليل حساسية وسرعة الماوس بنسبة 70% فوراً. مثالي جداً لتحديد نصوص صغيرة، أو الضغط على أزرار دقيقة في المتصفح.

---

### 4️⃣ أزرار التحكم بالكيبورد الشفاف الافتراضي (Virtual Keyboard Mode)

عند الضغط على **L3** يظهر الكيبورد الشفاف الأنيق على الشاشة، وتتحول اليد فوراً إلى وضع الكتابة الاحترافي:

| الزر أثناء ظهور الكيبورد | الوظيفة (Function) |
| :--- | :--- |
| **🕹️ عصا الأنالوج أو 🎛️ D-Pad** | **تحريك المؤشر بين الحروف** في جميع الاتجاهات بسلاسة وسرعة استجابة فائقة. |
| **🟢 زر A** | **طباعة الحرف المحدد (Type Character)** مع صوت نقرة ميكانيكية ممتعة. |
| **🔴 زر B** | **مسح الحرف السابق (Backspace)** مع إمكانية التعليق للمسح السريع. |
| **🔵 زر X** | **مسافة (Space Bar)**. |
| **🟡 زر Y** | **تبديل اللغة (Switch Arabic ⇋ English)** بضغطة زر واحدة. |
| **🎯 الزناد الأيسر (LT)** | **الحركات والتشكيل العربي / Shift:** يظهر حركات التشكيل كاملة (`َ ِ ُ ً ٍ ٌ ّ ْ`)، أو الحروف الكبيرة بالإنجليزية. |
| **⚡ الزناد الأيمن (RT)** | **طبقة الأرقام والرموز:** التبديل الفوري لطبقة الأرقام والرموز الخاصة (`@ # $ %`). |
| **⏮️ LB / ⏭️ RB** | **تحريك مؤشر الكتابة (Move Caret):** تحريك مؤشر الكتابة داخل النص لليمين أو اليسار دون الحاجة للمس الفأرة. |
| **▶️ زر Start** | **سطر جديد (Enter)**. |
| **🔘 ضغطة L3** | **إغلاق الكيبورد** والعودة فوراً لوضع الماوس العادي. |
| **🔘 ضغطة R3** | **إظهار / إخفاء شريط المساعدة والإرشادات** في الكيبورد. |
| **🎮 L3 + R3 معاً** | **تبديل نمط الشطرين (Dual-Stick Split Mode):** تقسيم الكيبورد لنصفين بحيث يتحكم الأنالوج الأيسر بنصف الكيبورد والأيمن بالنصف الآخر للكتابة السريعة بيدين! |

---

### 5️⃣ التحكم بمبدل المهام ثلاثي الأبعاد (Task Switcher Assist)

عند ظهور مبدل المهام (Alt+Tab أو Task View)، يقوم البرنامج تلقائياً بتمكين وضع التنقل السريع:
- **عصا الأنالوج / الأسهم / LB / RB:** التنقل الفوري بين النوافذ المفتوحة مع هزة تفاعلية نقية لكل نافذة.
- **🟢 زر A:** اختيار النافذة وتكبيرها فوراً (`Enter`).
- **🔵 زر X:** إغلاق النافذة المحددة فوراً (`Delete`).
- **🔴 زر B:** إلغاء المبدل والرجوع للنافذة الحالية (`Escape`).

---

### 6️⃣ القائمة الدائرية السريعة (Radial 8-Way Pie Menu)

عند الضغط على **Back + R3**، تظهر قائمة دائرية حديثة حول مؤشر الفأرة تحتوي على 8 قطاعات سريعة:
- حرك عصا الأنالوج في أي من الاتجاهات الـ 8 وافلت الزر للتنفيذ الفوري:
  - **شمال (Up):** مستكشف الملفات (Windows Explorer).
  - **يمين (Right):** متصفح الإنترنت الافتراضي.
  - **جنوب (Down):** إعدادات البرنامج (Settings).
  - **يسار (Left):** عرض المهام وسطح المكتب الافتراضي (Task View).
  - **الزوايا القطرية:** لقطة شاشة، كتم الصوت، تبديل الملفات الشخصية.

---

## 🚀 Getting Started (التشغيل السريع)

### Download Pre-built Release
قم بتحميل النسخة الجاهزة مباشرة من صفحة الإصدارات:
👉 **[Download Latest Release (v1.0.0)](https://github.com/m7kor/Reverse-Controller/releases/latest)**

- التطبيق يعمل كبرنامج محمول (Portable) لا يتطلب أي تثبيت:
  1. شغّل `ReverseController.exe`.
  2. وصل يد التحكم واستمتع بتجربة سطح مكتب متكاملة!
  3. للنقر على أيقونة التراي (بجانب الساعة):
     - **نقرة يسار:** تفتح نافذة الإعدادات.
     - **نقرة يمين:** تفتح القائمة السريعة مع زر تشغيل/إيقاف التحكم.

---

## 🛠️ Building From Source (طريقة البناء من المصدر)

### Prerequisites
- Windows 10 أو Windows 11 (x64)
- [Rust toolchain (stable MSVC)](https://rustup.rs/)
- Visual Studio C++ Build Tools, Windows SDK, CMake, and LLVM/libclang for the default voice build.
- Offline dictation needs a multilingual `ggml-base.bin` or `ggml-tiny.bin` model in `%APPDATA%\ControllerCompanionRust\models`.

```powershell
# استنساخ المستودع
git clone https://github.com/m7kor/Reverse-Controller.git
cd Reverse-Controller

# بناء النسخة المحسنة النهائية (Release Build)
cargo build --release --features "gui,voice"
```
الملف التنفيذي الناتج سيكون متاحاً في المسار:
`target/release/controller-companion.exe`

### تشغيل الاختبارات البرمجية (Run Tests)
لتشغيل الاختبارات البرمجية:
```powershell
cargo test --features "gui,voice"
```

Run `powershell -File scripts/check.ps1` for full-feature tests, headless tests,
and strict static analysis. Use `-WithoutVoice` when voice build tools are unavailable.
The interactive pointer test is excluded by default; run it explicitly with
`cargo test --all-features test_send_input_mouse -- --ignored` on an interactive desktop.

### Diagnostics and movement presets

- Virtual keyboard: choose **Edit** in the bottom row for a controller-accessible
  editing panel (Copy, Paste, Cut, Select all, Undo, Redo, Delete word, Delete,
  Home/End and arrows). Choose **ABC / حروف** to return. Labels follow the
  keyboard language. Redo sends Ctrl+Y; Delete word sends Ctrl+Backspace, with
  behavior determined by the target application.
- Arabic predictions retain lam-alef and diacritics, and stop at Arabic punctuation.
  Moving the caret with keyboard actions or switching foreground windows clears
  the old prediction context. The local preview is not a full mirror of the target text field.

- Settings → Pointer offers **Precision**, **Balanced**, and **Large display** presets. Save to apply.
- **Measure stick drift** guides a four-second rest calibration. Release both sticks;
  review the proposed dead zones, choose **Use suggested dead zones**, then Save.
  Disconnection, controller-slot changes, excessive movement, or too few samples
  cancel the measurement without changing your settings.
- `controller-companion.exe --config path\config.json --validate-config` checks syntax and numeric ranges without starting controller input; invalid files return exit code 2.
- `controller-companion.exe --status` reports all four XInput slots without changing Windows gamepad navigation settings.
- Configuration and profile saves replace the destination only after the complete new file is written. Failed replacement leaves the previous file intact.

---

## ⚙️ التخصيص وملفات الإعدادات (Configuration)

يتم تخزين إعدادات البرنامج وحساسية الماوس وتخطيط الأزرار بصيغة JSON نظيفة في المسار:
`%APPDATA%\ControllerCompanionRust\config.json`

نموذج لجزء من ملف الإعدادات:
```json
{
  "enabled": true,
  "disable_in_fullscreen": true,
  "autostart": false,
  "vk_auto_show": true,
  "toggle_hotkey": "Ctrl+K",
  "mouse": {
    "speed": 450.0,
    "acceleration": 800.0,
    "wheel_speed": 3600.0
  }
}
```

---

## Performance verification

RAM, GPU usage, startup time, and input latency have not been benchmarked reproducibly for this revision. Earlier numerical comparisons should not be treated as release guarantees. See [review and acceptance checks](docs/REVIEW_2026-10-16.md).

---

## 📜 License

Distributed under the MIT License. See [LICENSE](LICENSE) for more information.

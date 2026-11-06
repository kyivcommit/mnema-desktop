// Generates the image-only ("scanned") PDF fixtures for the OCR tests.
//
// Run from this directory, on macOS:
//
//     swift make_scan_fixtures.swift
//
// Writes scan-uk.pdf, scan-two-columns.pdf, mixed-native-scan.pdf and
// scan-six-pages.pdf next to the script — or only the ones named:
//
//     swift make_scan_fixtures.swift scan-six-pages.pdf
//
// All text is synthetic.
//
// A scan page is a bitmap drawn at 300 dpi (white background, black text) and
// then placed in the PDF as a picture: the page has no text objects and no
// fonts, which is what a real scanner produces. The one page in
// mixed-native-scan.pdf that is NOT a picture is drawn with CoreText straight
// into the PDF context, which embeds the font and writes a real text layer.
//
// Not reproducible byte for byte (CoreGraphics stamps a creation date), so
// the PDFs are committed and this script is only needed to change them.

import CoreGraphics
import CoreText
import Foundation

let dpi: CGFloat = 300
let pageW: CGFloat = 595   // A4, points
let pageH: CGFloat = 842
let fontName = "Times New Roman"   // ships with macOS and carries Cyrillic
let fontSize: CGFloat = 12
let leading: CGFloat = 20

let font = CTFontCreateWithName(fontName as CFString, fontSize, nil)

func line(_ text: String) -> CTLine {
    let attrs: [CFString: Any] = [
        kCTFontAttributeName: font,
        kCTForegroundColorAttributeName: CGColor(gray: 0, alpha: 1),
    ]
    return CTLineCreateWithAttributedString(NSAttributedString(string: text, attributes: attrs as [NSAttributedString.Key: Any]))
}

/// Draws `lines` top to bottom, the first baseline `top` points below the page top.
func draw(_ lines: [String], x: CGFloat, top: CGFloat, in ctx: CGContext) {
    for (i, text) in lines.enumerated() {
        ctx.textPosition = CGPoint(x: x, y: pageH - top - CGFloat(i) * leading)
        CTLineDraw(line(text), ctx)
    }
}

/// One page of text as a gray bitmap at 300 dpi, drawn in page points.
func scan(_ draws: (CGContext) -> Void) -> CGImage {
    let w = Int(pageW * dpi / 72), h = Int(pageH * dpi / 72)
    let ctx = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: 0,
                        space: CGColorSpaceCreateDeviceGray(), bitmapInfo: CGImageAlphaInfo.none.rawValue)!
    ctx.setFillColor(CGColor(gray: 1, alpha: 1))
    ctx.fill(CGRect(x: 0, y: 0, width: w, height: h))
    ctx.scaleBy(x: dpi / 72, y: dpi / 72)
    draws(ctx)
    return ctx.makeImage()!
}

/// The fixtures named on the command line, or all of them.
let only = Set(CommandLine.arguments.dropFirst())

func writePDF(_ name: String, pages: [(CGContext) -> Void]) {
    if !only.isEmpty && !only.contains(name) { return }
    var box = CGRect(x: 0, y: 0, width: pageW, height: pageH)
    let ctx = CGContext(URL(fileURLWithPath: name) as CFURL, mediaBox: &box, nil)!
    for page in pages {
        ctx.beginPDFPage(nil)
        page(ctx)
        ctx.endPDFPage()
    }
    ctx.closePDF()
}

/// A page that is only a picture of `draws`.
func scanPage(_ draws: @escaping (CGContext) -> Void) -> (CGContext) -> Void {
    let image = scan(draws)
    return { $0.draw(image, in: CGRect(x: 0, y: 0, width: pageW, height: pageH)) }
}

// Every assertion the tests make about these words is exact, so they are
// plain, high-contrast and hyphen-free. The apostrophe is U+2019.
let ukLines = [
    "Тоді їжак і єнот зустрілися біля старого ґанку бібліотеки.",
    "На столі лежали хліб, сир та м’ясо, а поруч пам’ятна книга.",
    "Староста сказав, що слово collegium означає спільноту людей.",
    "Вечірнє світло лягало на полиці, і тиша була майже чутною.",
    "Дрібний ґудзик від пальта знайшовся під шафою біля вікна.",
    "Усі записи перенесли до нової зали, щоб їх було легше читати.",
    "Країна має давні традиції письма, і це видно в кожній сторінці.",
    "Кінець абзацу: простий текст для перевірки розпізнавання.",
]

let leftLines = [
    "ЛІВА 1 Київ стоїть на високому березі",
    "ЛІВА 2 Дніпра і славиться каштанами",
    "ЛІВА 3 старі парки ховають мости",
    "ЛІВА 4 просторі площі та ринки",
    "ЛІВА 5 тихі двори біля храмів",
    "ЛІВА 6 зелені сади над річкою",
    "ЛІВА 7 вузькі вулиці йдуть вгору",
    "ЛІВА 8 і спускаються до води",
]
let rightLines = [
    "ПРАВА 1 річка широка тече повільно",
    "ПРАВА 2 повз золоті куполи восени",
    "ПРАВА 3 навесні береги світлі",
    "ПРАВА 4 влітку пливуть кораблі",
    "ПРАВА 5 взимку лід тримає човни",
    "ПРАВА 6 вночі мости горять вогнями",
    "ПРАВА 7 вдень лунають дзвони",
    "ПРАВА 8 ввечері місто стихає",
]

let nativeLines = [
    "Перша сторінка має справжній текстовий шар.",
    "Другу сторінку цього файлу відскановано як картинку.",
    "Native page one carries a real text layer for the reader.",
]

writePDF("scan-uk.pdf", pages: [scanPage { draw(ukLines, x: 60, top: 90, in: $0) }])

// The columns are far apart and the right one starts at the same height, so a
// reader that sorts by baseline and not by column interleaves them.
writePDF("scan-two-columns.pdf", pages: [scanPage {
    draw(leftLines, x: 60, top: 90, in: $0)
    draw(rightLines, x: 330, top: 90, in: $0)
}])

writePDF("mixed-native-scan.pdf", pages: [
    { draw(nativeLines, x: 60, top: 90, in: $0) },
    scanPage { draw(ukLines, x: 60, top: 90, in: $0) },
])

// Six scanned pages, each saying which page it is, for the parallel OCR tests
// (D196): with several helpers the pages are recognised out of order, and
// each one's own number on it is what shows its text came back to its place.
let sixPages: [[String]] = (1...6).map { n in [
    "Сторінка \(n) з шести: перевірка порядку сторінок.",
    "Номер \(n) стоїть тут, щоб текст мав своє місце.",
    "Page \(n) of six, a synthetic scan for the reader.",
] }
writePDF("scan-six-pages.pdf", pages: sixPages.map { lines in
    scanPage { draw(lines, x: 60, top: 90, in: $0) }
})

// Generates the image-only ("scanned") PDF fixtures for the OCR tests.
//
// Run from this directory, on macOS:
//
//     swift make_scan_fixtures.swift
//
// Writes scan-uk.pdf, scan-two-columns.pdf, mixed-native-scan.pdf,
// scan-six-pages.pdf, scan-rotated-90.pdf and the image-*.{png,jpg,heic,tif}
// fixtures plus not-heif.mp4 next to the script — or only the ones named:
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
import ImageIO
import PDFKit

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

/// Draws `lines` top to bottom, the first baseline `top` points below the top
/// of a surface `height` points tall (a page, unless an image card says otherwise).
func draw(_ lines: [String], x: CGFloat, top: CGFloat, in ctx: CGContext, height: CGFloat = pageH) {
    for (i, text) in lines.enumerated() {
        ctx.textPosition = CGPoint(x: x, y: height - top - CGFloat(i) * leading)
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

// A scanner that fed the sheet sideways: the picture lies on its side in a
// landscape MediaBox, and `/Rotate 90` (clockwise) is what stands it upright.
// Displayed, it is the same portrait A4 as the others, its text where `draw`
// puts it. CoreGraphics cannot write `/Rotate`, so PDFKit sets it on the page.
let rotatedLines = [
    "ПОВОРОТ один: рядок угорі сторінки для рамки.",
    "ПОВОРОТ два: другий рядок стоїть нижче першого.",
]
if only.isEmpty || only.contains("scan-rotated-90.pdf") {
    let mediaW = pageH, mediaH = pageW   // landscape
    let w = Int(mediaW * dpi / 72), h = Int(mediaH * dpi / 72)
    let bitmap = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: 0,
                           space: CGColorSpaceCreateDeviceGray(), bitmapInfo: CGImageAlphaInfo.none.rawValue)!
    bitmap.setFillColor(CGColor(gray: 1, alpha: 1))
    bitmap.fill(CGRect(x: 0, y: 0, width: w, height: h))
    bitmap.scaleBy(x: dpi / 72, y: dpi / 72)
    // Displayed (X, Y) lands at media (mediaW - Y, X): the inverse of the
    // clockwise turn `/Rotate 90` makes when the page is shown.
    bitmap.translateBy(x: mediaW, y: 0)
    bitmap.rotate(by: .pi / 2)
    draw(rotatedLines, x: 60, top: 90, in: bitmap)
    let image = bitmap.makeImage()!

    let data = NSMutableData()
    var box = CGRect(x: 0, y: 0, width: mediaW, height: mediaH)
    let ctx = CGContext(consumer: CGDataConsumer(data: data as CFMutableData)!, mediaBox: &box, nil)!
    ctx.beginPDFPage(nil)
    ctx.draw(image, in: box)
    ctx.endPDFPage()
    ctx.closePDF()
    let doc = PDFDocument(data: data as Data)!
    doc.page(at: 0)!.rotation = 90
    precondition(doc.write(to: URL(fileURLWithPath: "scan-rotated-90.pdf")), "write scan-rotated-90.pdf")
}

// ---- Image files (the image reader). The same `ukLines` as scan-uk.pdf, on a
// card the width of an A4 page and only as tall as the text, so each file
// stays small. 300 dpi, black on white, except where a fixture says otherwise.
//
// image-uk.webp is not written here — ImageIO reads WebP but cannot write it:
//
//     cwebp -quiet -q 85 image-uk.png -o image-uk.webp

let cardH: CGFloat = 280

enum Surface { case rgb, gray, transparent }

/// `lines` on a card, as an image of the given kind.
func card(_ lines: [String], _ surface: Surface = .rgb) -> CGImage {
    let w = Int(pageW * dpi / 72), h = Int(cardH * dpi / 72)
    let ctx: CGContext
    switch surface {
    case .rgb, .transparent:
        ctx = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: 0,
                        space: CGColorSpaceCreateDeviceRGB(),
                        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    case .gray:
        ctx = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: 0,
                        space: CGColorSpaceCreateDeviceGray(), bitmapInfo: CGImageAlphaInfo.none.rawValue)!
    }
    if surface == .transparent {
        ctx.clear(CGRect(x: 0, y: 0, width: w, height: h))
    } else {
        ctx.setFillColor(CGColor(gray: 1, alpha: 1))
        ctx.fill(CGRect(x: 0, y: 0, width: w, height: h))
    }
    ctx.scaleBy(x: dpi / 72, y: dpi / 72)
    draw(lines, x: 60, top: 50, in: ctx, height: cardH)
    return ctx.makeImage()!
}

/// `image` turned a quarter turn counter-clockwise: what a camera held sideways
/// stores, and what `Orientation = 6` tells a viewer to turn back.
func turnedCounterClockwise(_ image: CGImage) -> CGImage {
    let ctx = CGContext(data: nil, width: image.height, height: image.width, bitsPerComponent: 8,
                        bytesPerRow: 0, space: CGColorSpaceCreateDeviceRGB(),
                        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    ctx.translateBy(x: CGFloat(image.height), y: 0)
    ctx.rotate(by: .pi / 2)
    ctx.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
    return ctx.makeImage()!
}

/// A picture with no text: a few soft shapes on a gradient, like a photo.
func blankPhoto(_ w: Int = 1000, _ h: Int = 750) -> CGImage {
    let ctx = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: 0,
                        space: CGColorSpaceCreateDeviceRGB(),
                        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    let gradient = CGGradient(colorsSpace: CGColorSpaceCreateDeviceRGB(),
                              colors: [CGColor(red: 0.2, green: 0.4, blue: 0.7, alpha: 1),
                                       CGColor(red: 0.9, green: 0.8, blue: 0.5, alpha: 1)] as CFArray,
                              locations: [0, 1])!
    ctx.drawLinearGradient(gradient, start: .zero, end: CGPoint(x: 0, y: h), options: [])
    ctx.setFillColor(CGColor(red: 0.1, green: 0.5, blue: 0.2, alpha: 1))
    ctx.fillEllipse(in: CGRect(x: 120, y: 80, width: 420, height: 260))
    ctx.setFillColor(CGColor(red: 0.8, green: 0.3, blue: 0.2, alpha: 0.8))
    ctx.fillEllipse(in: CGRect(x: 560, y: 300, width: 300, height: 300))
    return ctx.makeImage()!
}

/// Writes `frames` to `name` as `type`, each with its own properties.
func writeImage(_ name: String, _ type: String, _ frames: [(CGImage, [CFString: Any])]) {
    if !only.isEmpty && !only.contains(name) { return }
    let dst = CGImageDestinationCreateWithURL(URL(fileURLWithPath: name) as CFURL, type as CFString,
                                              frames.count, nil)!
    for (image, props) in frames { CGImageDestinationAddImage(dst, image, props as CFDictionary) }
    precondition(CGImageDestinationFinalize(dst), "write \(name)")
}

// 0.6 rather than a camera's ~0.85: the text survives it (measured with the
// reader), and the files stay small enough to commit.
let lossy: [CFString: Any] = [kCGImageDestinationLossyCompressionQuality: 0.6]
// LZW (5): uncompressed, the three frames are 17 MB.
let lzw: [CFString: Any] = [kCGImagePropertyTIFFDictionary: [kCGImagePropertyTIFFCompression: 5]]
writeImage("image-uk.png", "public.png", [(card(ukLines), [:])])
writeImage("image-uk.jpg", "public.jpeg", [(card(ukLines), lossy)])
writeImage("image-uk.heic", "public.heic", [(card(ukLines), lossy)])
// ImageIO hands a gray file back as 8 bits per pixel; Vision reads nothing from
// a gray bitmap given as-is (`ocr.rs`), so this is the reader's redraw to BGRA
// under test.
writeImage("image-uk-gray.png", "public.png", [(card(ukLines, .gray), [:])])
// Black text on nothing: drawn onto black, it would vanish.
writeImage("image-uk-transparent.png", "public.png", [(card(ukLines, .transparent), [:])])
var turned = lossy
turned[kCGImagePropertyOrientation] = 6
writeImage("image-uk-exif6.jpg", "public.jpeg", [(turnedCounterClockwise(card(ukLines)), turned)])
writeImage("image-blank.jpg", "public.jpeg", [(blankPhoto(), lossy)])
// Legible, and short of the 48 characters a page needs: a sign, a caption.
writeImage("image-short.png", "public.png", [(card(["їжак і єнот"]), [:])])
// Text, no text, text: the middle frame is the one a reader must skip and name.
writeImage("image-three-frames.tif", "public.tiff", [
    (card(sixPages[0], .gray), lzw),
    (blankPhoto(), lzw),
    (card(sixPages[2], .gray), lzw),
])

// An animated PNG: three frames of the same picture, which is what an
// animation's frames are — the reader must count it as one page.
writeImage("image-animated.png", "public.png", [
    (blankPhoto(100, 75), [:]), (blankPhoto(100, 75), [:]), (blankPhoto(100, 75), [:]),
])

// The magic an HEIF file shares with video: `ftyp` at offset 4. Only the brand
// (`isom` here, MP4) says it is not a picture.
if only.isEmpty || only.contains("not-heif.mp4") {
    var bytes: [UInt8] = [0x00, 0x00, 0x00, 0x18]
    bytes += Array("ftypisom".utf8) + [0x00, 0x00, 0x02, 0x00] + Array("isomiso2".utf8)
    bytes += [UInt8](repeating: 0, count: 40)
    precondition(FileManager.default.createFile(atPath: "not-heif.mp4", contents: Data(bytes)), "write not-heif.mp4")
}

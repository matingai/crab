import Foundation
import PDFKit

struct PdfMetadata: Codable {
    let title: String?
    let author: String?
    let subject: String?
    let keywords: [String]
    let creator: String?
}

struct PdfPagePayload: Codable {
    let id: String
    let pageNumber: Int
    let label: String
    let charCount: Int
    let truncated: Bool
    let text: String
}

struct PdfInspectPayload: Codable {
    let path: String
    let format: String
    let kind: String
    let supported: Bool
    let capabilities: [String: Bool]
    let pageCount: Int
    let metadata: PdfMetadata
}

struct PdfPreviewPayload: Codable {
    let type: String
    let path: String
    let pageCount: Int
    let extractedPageCount: Int
    let truncated: Bool
    let metadata: PdfMetadata
    let pages: [PdfPagePayload]
}

struct PdfIrPayload: Codable {
    let type: String
    let path: String
    let pageCount: Int
    let extractedPageCount: Int
    let truncated: Bool
    let metadata: PdfMetadata
    let pages: [PdfPagePayload]
}

enum PdfHelperError: Error {
    case invalidArguments(String)
    case invalidPayload(String)
    case documentOpenFailed(String)
}

func stringValue(_ payload: [String: Any], _ key: String) -> String? {
    guard let value = payload[key] as? String else {
        return nil
    }
    let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
    return trimmed.isEmpty ? nil : trimmed
}

func intValue(_ payload: [String: Any], _ key: String, defaultValue: Int) -> Int {
    if let value = payload[key] as? Int {
        return value
    }
    if let value = payload[key] as? Double {
        return Int(value)
    }
    if let raw = payload[key] as? String, let parsed = Int(raw) {
        return parsed
    }
    return defaultValue
}

func metadata(from document: PDFDocument) -> PdfMetadata {
    let attributes = document.documentAttributes ?? [:]
    let title = attributes[PDFDocumentAttribute.titleAttribute] as? String
    let author = attributes[PDFDocumentAttribute.authorAttribute] as? String
    let subject = attributes[PDFDocumentAttribute.subjectAttribute] as? String
    let creator = attributes[PDFDocumentAttribute.creatorAttribute] as? String

    let keywords: [String]
    if let list = attributes[PDFDocumentAttribute.keywordsAttribute] as? [String] {
        keywords = list.map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }.filter { !$0.isEmpty }
    } else if let raw = attributes[PDFDocumentAttribute.keywordsAttribute] as? String {
        keywords = raw
            .split(separator: ",")
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { !$0.isEmpty }
    } else {
        keywords = []
    }

    return PdfMetadata(
        title: title?.trimmingCharacters(in: .whitespacesAndNewlines).nilIfEmpty,
        author: author?.trimmingCharacters(in: .whitespacesAndNewlines).nilIfEmpty,
        subject: subject?.trimmingCharacters(in: .whitespacesAndNewlines).nilIfEmpty,
        keywords: keywords,
        creator: creator?.trimmingCharacters(in: .whitespacesAndNewlines).nilIfEmpty
    )
}

func sanitizeText(_ raw: String) -> String {
    let normalized = raw
        .replacingOccurrences(of: "\r\n", with: "\n")
        .replacingOccurrences(of: "\r", with: "\n")
        .replacingOccurrences(of: "\u{0}", with: "")

    let compactedLines = normalized
        .split(separator: "\n", omittingEmptySubsequences: false)
        .map { line in
            line
                .replacingOccurrences(of: #"\s+"#, with: " ", options: .regularExpression)
                .trimmingCharacters(in: .whitespacesAndNewlines)
        }

    var result: [String] = []
    var blankRun = 0
    for line in compactedLines {
        if line.isEmpty {
            blankRun += 1
            if blankRun <= 2 {
                result.append("")
            }
        } else {
            blankRun = 0
            result.append(line)
        }
    }

    return result
        .joined(separator: "\n")
        .trimmingCharacters(in: .whitespacesAndNewlines)
}

func excerpt(_ text: String, maxChars: Int) -> (String, Bool) {
    if maxChars <= 0 {
        return ("", !text.isEmpty)
    }
    if text.count <= maxChars {
        return (text, false)
    }
    let endIndex = text.index(text.startIndex, offsetBy: maxChars)
    return (String(text[..<endIndex]).trimmingCharacters(in: .whitespacesAndNewlines), true)
}

func extractPages(document: PDFDocument, maxPages: Int, maxCharsPerPage: Int) -> ([PdfPagePayload], Bool) {
    let pageCount = document.pageCount
    let limit = max(0, min(maxPages, pageCount))
    var pages: [PdfPagePayload] = []
    var truncated = pageCount > limit

    for pageIndex in 0..<limit {
        guard let page = document.page(at: pageIndex) else {
            continue
        }
        let sanitized = sanitizeText(page.string ?? "")
        let (text, pageTruncated) = excerpt(sanitized, maxChars: maxCharsPerPage)
        truncated = truncated || pageTruncated
        pages.append(
            PdfPagePayload(
                id: "page-\(pageIndex + 1)",
                pageNumber: pageIndex + 1,
                label: page.label?.trimmingCharacters(in: .whitespacesAndNewlines).nilIfEmpty ?? "\(pageIndex + 1)",
                charCount: sanitized.count,
                truncated: pageTruncated,
                text: text
            )
        )
    }

    return (pages, truncated)
}

func loadDocument(path: String) throws -> PDFDocument {
    let url = URL(fileURLWithPath: path)
    guard let document = PDFDocument(url: url) else {
        throw PdfHelperError.documentOpenFailed("failed to open pdf: \(path)")
    }
    return document
}

func inspectDocument(path: String) throws -> PdfInspectPayload {
    let document = try loadDocument(path: path)
    return PdfInspectPayload(
        path: path,
        format: "pdf",
        kind: "document",
        supported: true,
        capabilities: [
            "preview": true,
            "extract_ir": true,
            "create": false,
            "apply_ops": false,
        ],
        pageCount: document.pageCount,
        metadata: metadata(from: document)
    )
}

func previewDocument(path: String, maxPages: Int, maxCharsPerPage: Int) throws -> PdfPreviewPayload {
    let document = try loadDocument(path: path)
    let (pages, truncated) = extractPages(document: document, maxPages: maxPages, maxCharsPerPage: maxCharsPerPage)
    return PdfPreviewPayload(
        type: "pdf_preview",
        path: path,
        pageCount: document.pageCount,
        extractedPageCount: pages.count,
        truncated: truncated,
        metadata: metadata(from: document),
        pages: pages
    )
}

func extractDocumentIr(path: String, maxPages: Int, maxCharsPerPage: Int) throws -> PdfIrPayload {
    let document = try loadDocument(path: path)
    let (pages, truncated) = extractPages(document: document, maxPages: maxPages, maxCharsPerPage: maxCharsPerPage)
    return PdfIrPayload(
        type: "pdf",
        path: path,
        pageCount: document.pageCount,
        extractedPageCount: pages.count,
        truncated: truncated,
        metadata: metadata(from: document),
        pages: pages
    )
}

func main() throws {
    guard CommandLine.arguments.count >= 2 else {
        throw PdfHelperError.invalidArguments("missing action")
    }

    let payloadData = FileHandle.standardInput.readDataToEndOfFile()
    let payloadObject = try JSONSerialization.jsonObject(with: payloadData, options: [])
    guard let payload = payloadObject as? [String: Any] else {
        throw PdfHelperError.invalidPayload("expected JSON object payload")
    }
    guard let path = stringValue(payload, "path") else {
        throw PdfHelperError.invalidPayload("missing path")
    }

    let action = CommandLine.arguments[1]
    let encoder = JSONEncoder()

    switch action {
    case "inspect":
        try FileHandle.standardOutput.write(contentsOf: encoder.encode(inspectDocument(path: path)))
    case "preview":
        let maxPages = max(1, intValue(payload, "max_pages", defaultValue: 8))
        let maxCharsPerPage = max(200, intValue(payload, "max_chars_per_page", defaultValue: 1200))
        try FileHandle.standardOutput.write(
            contentsOf: encoder.encode(
                previewDocument(path: path, maxPages: maxPages, maxCharsPerPage: maxCharsPerPage)
            )
        )
    case "extract_ir":
        let maxPages = max(1, intValue(payload, "max_pages", defaultValue: 20))
        let maxCharsPerPage = max(200, intValue(payload, "max_chars_per_page", defaultValue: 4000))
        try FileHandle.standardOutput.write(
            contentsOf: encoder.encode(
                extractDocumentIr(path: path, maxPages: maxPages, maxCharsPerPage: maxCharsPerPage)
            )
        )
    default:
        throw PdfHelperError.invalidArguments("unsupported action: \(action)")
    }
}

extension String {
    var nilIfEmpty: String? {
        isEmpty ? nil : self
    }
}

do {
    try main()
} catch {
    let output = ["error": String(describing: error)]
    let data = try JSONSerialization.data(withJSONObject: output, options: [])
    try FileHandle.standardOutput.write(contentsOf: data)
    exit(1)
}

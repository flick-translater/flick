#!/usr/bin/env swift

import Vision
import Foundation
import CoreGraphics

func recognizeText(imagePath: String) -> String {
    let url = URL(fileURLWithPath: imagePath)
    
    guard let imageData = try? Data(contentsOf: url),
          let cgImageSource = CGImageSourceCreateWithData(imageData as CFData, nil),
          let cgImage = CGImageSourceCreateImageAtIndex(cgImageSource, 0, nil) else {
        return ""
    }
    
    let requestHandler = VNImageRequestHandler(cgImage: cgImage, options: [:])
    let textRequest = VNRecognizeTextRequest()
    
    textRequest.recognitionLevel = .accurate
    textRequest.recognitionLanguages = ["zh-Hans", "zh-Hant", "en", "ja"]
    textRequest.usesLanguageCorrection = true
    textRequest.regionOfInterest = CGRect(x: 0, y: 0, width: 1, height: 1)
    
    do {
        try requestHandler.perform([textRequest])
    } catch {
        return ""
    }
    
    guard let observations = textRequest.results else {
        return ""
    }
    
    var texts: [String] = []
    texts.reserveCapacity(observations.count)
    
    for observation in observations {
        if let candidate = observation.topCandidates(1).first,
           !candidate.string.isEmpty {
            texts.append(candidate.string)
        }
    }
    
    return texts.joined(separator: "\n")
}

if CommandLine.arguments.count < 2 {
    print("Usage: ocr-tool <image_path>", terminator: "")
    exit(1)
}

let imagePath = CommandLine.arguments[1]
let result = recognizeText(imagePath: imagePath)
print(result, terminator: "")
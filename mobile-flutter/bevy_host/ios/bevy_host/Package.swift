// swift-tools-version: 5.9

import PackageDescription

let package = Package(
    name: "bevy_host",
    platforms: [
        .iOS("15.0")
    ],
    products: [
        .library(name: "bevy-host", targets: ["bevy_host"])
    ],
    dependencies: [
        .package(name: "FlutterFramework", path: "../FlutterFramework")
    ],
    targets: [
        // The Rust static library and its C header, built by scripts/build-ios-native.sh.
        .binaryTarget(name: "BevyHostCore", path: "BevyHostCore.xcframework"),
        .target(
            name: "bevy_host",
            dependencies: [
                "BevyHostCore",
                .product(name: "FlutterFramework", package: "FlutterFramework"),
            ],
            linkerSettings: [
                // A Rust staticlib doesn't carry its framework dependencies; these are what
                // wgpu's Metal backend and objc2 link against.
                .linkedFramework("Metal"),
                .linkedFramework("QuartzCore"),
                .linkedFramework("UIKit"),
                .linkedFramework("CoreGraphics"),
                .linkedFramework("CoreFoundation"),
                .linkedFramework("Foundation"),
                .linkedFramework("Security"),
                .linkedFramework("IOSurface"),
                .linkedLibrary("objc"),
                .linkedLibrary("c++"),
            ]
        ),
    ]
)

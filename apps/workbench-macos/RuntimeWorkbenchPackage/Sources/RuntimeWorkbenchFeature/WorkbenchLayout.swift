import CoreGraphics
import CryptoKit
import Foundation

public enum WorkbenchLayoutMode: String, CaseIterable, Codable, Hashable, Sendable {
    case freeform
    case tiling
    /// The selected window fills the canvas edge-to-edge with no per-window
    /// chrome. On iOS, opening another napplet while in this mode pushes a
    /// new screen onto a navigation stack instead of adding a canvas window.
    case fullWindow

    public var title: String {
        switch self {
        case .freeform: "Freeform"
        case .tiling: "Tiling"
        case .fullWindow: "Full Window"
        }
    }

    public var systemImage: String {
        switch self {
        case .freeform: "macwindow.on.rectangle"
        case .tiling: "rectangle.split.2x2"
        case .fullWindow: "rectangle.fill"
        }
    }
}

/// A stable component identity projected into the workspace.
///
/// This is a string-backed value instead of a closed enum so the canvas can
/// host exact builds discovered after the Workbench shipped.
public struct WorkbenchComponentID:
    RawRepresentable,
    Codable,
    Hashable,
    Sendable
{
    public let rawValue: String

    public init(rawValue: String) {
        self.rawValue = rawValue
    }

    public init(from decoder: any Decoder) throws {
        let container = try decoder.singleValueContainer()
        rawValue = try container.decode(String.self)
    }

    public func encode(to encoder: any Encoder) throws {
        var container = encoder.singleValueContainer()
        try container.encode(rawValue)
    }

    public static let goodMorning = Self(rawValue: "good-morning")
}

public struct WorkbenchWindowID:
    RawRepresentable,
    Codable,
    Hashable,
    Sendable
{
    public let rawValue: String

    public init(rawValue: String) {
        self.rawValue = rawValue
    }
}

public struct WorkbenchWindowFrame: Codable, Equatable, Sendable {
    public static let minimumWidth = 320.0
    public static let minimumHeight = 240.0
    public static let maximumWidth = 1_600.0
    public static let maximumHeight = 1_200.0
    public static let maximumCoordinate = 4_096.0

    public var x: Double
    public var y: Double
    public var width: Double
    public var height: Double

    public init(
        x: Double,
        y: Double,
        width: Double,
        height: Double
    ) {
        self.x = x
        self.y = y
        self.width = width
        self.height = height
    }

    public func bounded() -> Self {
        Self(
            x: min(max(x.isFinite ? x : 0, 0), Self.maximumCoordinate),
            y: min(max(y.isFinite ? y : 0, 0), Self.maximumCoordinate),
            width: min(
                max(width.isFinite ? width : Self.minimumWidth, Self.minimumWidth),
                Self.maximumWidth
            ),
            height: min(
                max(
                    height.isFinite ? height : Self.minimumHeight,
                    Self.minimumHeight
                ),
                Self.maximumHeight
            )
        )
    }

    func fitted(to canvasSize: CGSize) -> Self {
        let bounded = bounded()
        let availableWidth = max(Double(canvasSize.width), Self.minimumWidth)
        let availableHeight = max(Double(canvasSize.height), Self.minimumHeight)
        let width = min(bounded.width, availableWidth)
        let height = min(bounded.height, availableHeight)
        return Self(
            x: min(max(bounded.x, 0), max(availableWidth - width, 0)),
            y: min(max(bounded.y, 0), max(availableHeight - height, 0)),
            width: width,
            height: height
        )
    }
}

/// Exact verified build identity projected by Rust for workspace persistence.
public struct WorkbenchExactBuildIdentity:
    Codable,
    Equatable,
    Hashable,
    Sendable
{
    public let manifestAuthor: String
    public let dTag: String
    public let aggregateHash: String

    public init(
        manifestAuthor: String,
        dTag: String,
        aggregateHash: String
    ) {
        self.manifestAuthor = manifestAuthor
        self.dTag = dTag
        self.aggregateHash = aggregateHash
    }
}

public struct WorkbenchCanvasWindow:
    Codable,
    Equatable,
    Identifiable,
    Sendable
{
    public let id: WorkbenchWindowID
    public let componentID: WorkbenchComponentID
    public let exactBuild: WorkbenchExactBuildIdentity?
    public var title: String
    public var frame: WorkbenchWindowFrame
    public var stackingOrder: UInt16

    public init(
        id: WorkbenchWindowID,
        componentID: WorkbenchComponentID,
        exactBuild: WorkbenchExactBuildIdentity? = nil,
        title: String,
        frame: WorkbenchWindowFrame,
        stackingOrder: UInt16
    ) {
        self.id = id
        self.componentID = componentID
        self.exactBuild = exactBuild
        self.title = title
        self.frame = frame
        self.stackingOrder = stackingOrder
    }

    public static let goodMorning = Self(
        id: WorkbenchWindowID(rawValue: "good-morning"),
        componentID: .goodMorning,
        exactBuild: WorkbenchExactBuildIdentity(
            manifestAuthor: GoodMorningFixture.author,
            dTag: GoodMorningFixture.dTag,
            aggregateHash: GoodMorningFixture.aggregateHash
        ),
        title: "Good Morning",
        frame: WorkbenchWindowFrame(
            x: 40,
            y: 40,
            width: 760,
            height: 520
        ),
        stackingOrder: 0
    )

    public static func installed(
        title: String,
        identity: WorkbenchExactBuildIdentity,
        offset: Double
    ) -> Self {
        let stableMaterial =
            "\(identity.manifestAuthor)\u{0}\(identity.dTag)\u{0}\(identity.aggregateHash)"
        let stableDigest = SHA256.hash(data: Data(stableMaterial.utf8))
            .map { String(format: "%02x", $0) }
            .joined()
        let stableID = "napplet-\(stableDigest)"
        return Self(
            id: WorkbenchWindowID(rawValue: stableID),
            componentID: WorkbenchComponentID(rawValue: stableID),
            exactBuild: identity,
            title: title,
            frame: WorkbenchWindowFrame(
                x: 32 + offset,
                y: 32 + offset,
                width: 760,
                height: 520
            ),
            stackingOrder: 0
        )
    }
}

public struct WorkbenchLayoutSnapshot: Codable, Equatable, Sendable {
    public static let currentVersion = 2
    /// Mirrors the Rust workspace schema's persisted slot ceiling.
    public static let maximumWindowCount = 16

    public var version: Int
    public var mode: WorkbenchLayoutMode
    public var windows: [WorkbenchCanvasWindow]
    public var selectedWindowID: WorkbenchWindowID?

    public init(
        version: Int = currentVersion,
        mode: WorkbenchLayoutMode,
        windows: [WorkbenchCanvasWindow],
        selectedWindowID: WorkbenchWindowID?
    ) {
        self.version = version
        self.mode = mode
        self.windows = windows
        self.selectedWindowID = selectedWindowID
    }

    public static var workbenchDefault: WorkbenchLayoutSnapshot {
        WorkbenchLayoutSnapshot(
            mode: .freeform,
            windows: [],
            selectedWindowID: nil
        )
    }

    private enum CodingKeys: String, CodingKey {
        case version
        case mode
        case windows
        case selectedWindowID
        // Version 1 compatibility keys.
        case visibleRoles
        case assignments
        case focusedRole
        case sizes
    }

    public init(from decoder: any Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let decodedVersion = try container.decodeIfPresent(
            Int.self,
            forKey: .version
        ) ?? 1
        if decodedVersion == 1 {
            let assignments = try container.decodeIfPresent(
                [LegacySlotRole: WorkbenchComponentID].self,
                forKey: .assignments
            ) ?? [:]
            let sizes = try container.decodeIfPresent(
                [LegacySlotRole: LegacySlotSize].self,
                forKey: .sizes
            ) ?? [:]
            let assignedRole = LegacySlotRole.allCases.first {
                assignments[$0] == .goodMorning
            }
            let restoredSize = assignedRole.flatMap { sizes[$0] }
            if assignedRole != nil {
                var window = WorkbenchCanvasWindow.goodMorning
                if let restoredSize {
                    window.frame.width = restoredSize.width
                    window.frame.height = restoredSize.height
                }
                windows = [window]
                selectedWindowID = window.id
            } else {
                windows = []
                selectedWindowID = nil
            }
            version = Self.currentVersion
            mode = .freeform
            return
        }

        version = decodedVersion
        mode = try container.decodeIfPresent(
            WorkbenchLayoutMode.self,
            forKey: .mode
        ) ?? .freeform
        windows = try container.decodeIfPresent(
            [WorkbenchCanvasWindow].self,
            forKey: .windows
        ) ?? []
        selectedWindowID = try container.decodeIfPresent(
            WorkbenchWindowID.self,
            forKey: .selectedWindowID
        )
    }

    public func encode(to encoder: any Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(version, forKey: .version)
        try container.encode(mode, forKey: .mode)
        try container.encode(windows, forKey: .windows)
        try container.encodeIfPresent(
            selectedWindowID,
            forKey: .selectedWindowID
        )
    }
}

public struct WorkbenchLayoutModel: Equatable, Sendable {
    public private(set) var snapshot: WorkbenchLayoutSnapshot

    public init(snapshot: WorkbenchLayoutSnapshot = .workbenchDefault) {
        self.snapshot = Self.normalized(snapshot)
    }

    public var mode: WorkbenchLayoutMode {
        snapshot.mode
    }

    public var windows: [WorkbenchCanvasWindow] {
        snapshot.windows.sorted {
            if $0.stackingOrder == $1.stackingOrder {
                return $0.id.rawValue < $1.id.rawValue
            }
            return $0.stackingOrder < $1.stackingOrder
        }
    }

    public var selectedWindow: WorkbenchCanvasWindow? {
        guard let selectedWindowID = snapshot.selectedWindowID else {
            return nil
        }
        return snapshot.windows.first { $0.id == selectedWindowID }
    }

    public func window(
        id: WorkbenchWindowID
    ) -> WorkbenchCanvasWindow? {
        snapshot.windows.first { $0.id == id }
    }

    public mutating func setMode(_ mode: WorkbenchLayoutMode) {
        snapshot.mode = mode
    }

    public mutating func select(_ id: WorkbenchWindowID?) {
        guard
            id == nil || snapshot.windows.contains(where: { $0.id == id })
        else {
            return
        }
        snapshot.selectedWindowID = id
    }

    public mutating func bringToFront(_ id: WorkbenchWindowID) {
        guard let index = snapshot.windows.firstIndex(where: { $0.id == id }) else {
            return
        }
        let orderedIDs = windows.map(\.id).filter { $0 != id } + [id]
        for (order, orderedID) in orderedIDs.enumerated() {
            guard let orderedIndex = snapshot.windows.firstIndex(where: {
                $0.id == orderedID
            }) else {
                continue
            }
            snapshot.windows[orderedIndex].stackingOrder = UInt16(order)
        }
        snapshot.selectedWindowID = snapshot.windows[index].id
    }

    public mutating func moveWindow(
        id: WorkbenchWindowID,
        x: Double,
        y: Double,
        canvasSize: CGSize
    ) {
        guard let index = snapshot.windows.firstIndex(where: { $0.id == id }) else {
            return
        }
        var frame = snapshot.windows[index].frame
        frame.x = x
        frame.y = y
        snapshot.windows[index].frame = frame.fitted(to: canvasSize)
    }

    public mutating func resizeWindow(
        id: WorkbenchWindowID,
        width: Double,
        height: Double,
        canvasSize: CGSize
    ) {
        guard let index = snapshot.windows.firstIndex(where: { $0.id == id }) else {
            return
        }
        var frame = snapshot.windows[index].frame
        frame.width = width
        frame.height = height
        snapshot.windows[index].frame = frame.fitted(to: canvasSize)
    }

    public mutating func addWindow(_ window: WorkbenchCanvasWindow) -> Bool {
        guard
            snapshot.windows.count < WorkbenchLayoutSnapshot.maximumWindowCount,
            !snapshot.windows.contains(where: { $0.id == window.id })
        else {
            return false
        }
        var admitted = window
        admitted.frame = admitted.frame.bounded()
        admitted.stackingOrder = UInt16(snapshot.windows.count)
        snapshot.windows.append(admitted)
        snapshot.selectedWindowID = admitted.id
        return true
    }

    public mutating func removeWindow(id: WorkbenchWindowID) {
        snapshot.windows.removeAll { $0.id == id }
        if snapshot.selectedWindowID == id {
            snapshot.selectedWindowID = windows.last?.id
        }
    }

    private static func normalized(
        _ candidate: WorkbenchLayoutSnapshot
    ) -> WorkbenchLayoutSnapshot {
        guard candidate.version == WorkbenchLayoutSnapshot.currentVersion else {
            return .workbenchDefault
        }

        var result = candidate
        var seenIDs = Set<WorkbenchWindowID>()
        result.windows = Array(
            result.windows
                .filter { seenIDs.insert($0.id).inserted }
                .prefix(WorkbenchLayoutSnapshot.maximumWindowCount)
        )
        for index in result.windows.indices {
            result.windows[index].frame = result.windows[index].frame.bounded()
            result.windows[index].stackingOrder = UInt16(index)
            if result.windows[index].title.isEmpty {
                result.windows[index].title = "Napplet"
            }
        }

        if
            let selected = result.selectedWindowID,
            !result.windows.contains(where: { $0.id == selected })
        {
            result.selectedWindowID = result.windows.last?.id
        }
        if result.selectedWindowID == nil {
            result.selectedWindowID = result.windows.last?.id
        }
        return result
    }
}

private enum LegacySlotRole: String, CaseIterable, Codable {
    case feed
    case detail
    case composer
    case tool
}

private struct LegacySlotSize: Codable {
    let width: Double
    let height: Double
}

/// The Rust workspace adapter implements this protocol. The feature deliberately
/// has no UserDefaults, AppStorage, or SceneStorage fallback.
@MainActor
public protocol WorkbenchLayoutPersisting {
    func loadLayout(workspaceID: String) throws -> WorkbenchLayoutSnapshot?
    func saveLayout(
        _ snapshot: WorkbenchLayoutSnapshot,
        workspaceID: String
    ) throws
    func saveLayout(
        _ snapshot: WorkbenchLayoutSnapshot,
        workspaceID: String,
        retainedReceiptIDs: [String]
    ) throws
}

public extension WorkbenchLayoutPersisting {
    func saveLayout(
        _ snapshot: WorkbenchLayoutSnapshot,
        workspaceID: String,
        retainedReceiptIDs _: [String]
    ) throws {
        try saveLayout(snapshot, workspaceID: workspaceID)
    }
}

@MainActor
public struct VolatileWorkbenchLayoutStore: WorkbenchLayoutPersisting {
    public init() {}

    public func loadLayout(workspaceID: String) throws -> WorkbenchLayoutSnapshot? {
        nil
    }

    public func saveLayout(
        _ snapshot: WorkbenchLayoutSnapshot,
        workspaceID: String
    ) throws {}
}

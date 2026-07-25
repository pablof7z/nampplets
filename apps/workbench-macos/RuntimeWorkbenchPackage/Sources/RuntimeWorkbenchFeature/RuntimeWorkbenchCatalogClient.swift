import Foundation

/// Workbench adapter over one bounded live profile catalog.
///
/// `init()` is a truthful unavailable fallback for previews without a runtime
/// profile. The deterministic corpus is available only through
/// `offlineFixture()` and is always labeled as an offline UI-test source.
@MainActor
public final class RuntimeWorkbenchCatalogClient: CatalogClient {
    let profileBacking:
        (any RuntimeWorkbenchCatalogProfileBacking)?
    let records: [CatalogRecord]
    let isOfflineFixture: Bool
    let loadIssue: CatalogIssue?
    var activeLiveReviewID: String?

    public var feedScope: CatalogBrowseScope {
        profileBacking == nil ? .offlineFixture : .liveNMPWindow
    }

    public convenience init() {
        self.init(
            profileBacking: nil,
            records: [],
            isOfflineFixture: false,
            loadIssue: CatalogIssue(
                title: "Live catalog unavailable",
                message: "Open a runtime profile before browsing napplets."
            )
        )
    }

    public convenience init(
        profileBacking: any RuntimeWorkbenchCatalogProfileBacking
    ) {
        self.init(
            profileBacking: profileBacking,
            records: [],
            isOfflineFixture: false,
            loadIssue: nil
        )
    }

    public static func offlineFixture() -> RuntimeWorkbenchCatalogClient {
        RuntimeWorkbenchCatalogClient(offlineFixtureBundle: .module)
    }

    convenience init(offlineFixtureBundle bundle: Bundle) {
        do {
            self.init(
                profileBacking: nil,
                records: try Self.loadRecords(bundle: bundle),
                isOfflineFixture: true,
                loadIssue: nil
            )
        } catch {
            self.init(
                profileBacking: nil,
                records: [],
                isOfflineFixture: true,
                loadIssue: CatalogIssue(
                    title: "Offline fixture unavailable",
                    message: "The exact compatibility corpus bundled with this "
                        + "Workbench could not be loaded: "
                        + error.localizedDescription
                )
            )
        }
    }

    private init(
        profileBacking: (any RuntimeWorkbenchCatalogProfileBacking)?,
        records: [CatalogRecord],
        isOfflineFixture: Bool,
        loadIssue: CatalogIssue?
    ) {
        self.profileBacking = profileBacking
        self.records = records
        self.isOfflineFixture = isOfflineFixture
        self.loadIssue = loadIssue
    }
}

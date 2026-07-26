import SwiftUI

#if os(macOS)
extension ContentView {
    @ToolbarContentBuilder
    var macOSToolbar: some ToolbarContent {
        ToolbarItemGroup(placement: .automatic) {
            addNappletToolbarButton
            inspectorToolbarButton
            layoutMenu
        }

        // `#available` is a runtime check: the symbol still has to exist in the
        // SDK at compile time. `ToolbarSpacer` ships with the macOS 26 SDK, so
        // it must also be gated at parse time or the pinned Xcode 16.4
        // toolchain (Swift 6.1) fails with "cannot find 'ToolbarSpacer' in
        // scope". `#if compiler` is evaluated before type checking, so the body
        // is not compiled under 6.1 and is kept under Xcode 26's Swift 6.2+.
        #if compiler(>=6.2)
            if #available(macOS 26.0, *) {
                ToolbarSpacer(.flexible)
            }
        #endif

        ToolbarItem(placement: .primaryAction) {
            accountMenu
        }
    }

    private var addNappletToolbarButton: some View {
        Button {
            isCatalogSheetPresented = true
        } label: {
            Label("Add Napplet", systemImage: "plus")
        }
        .labelStyle(.iconOnly)
        .keyboardShortcut("n", modifiers: [.command])
        .help("Add Napplet")
        .accessibilityIdentifier("add-napplet")
        .accessibilityHint("Opens the napplet catalog")
    }

    private var inspectorToolbarButton: some View {
        Button {
            withAnimation(.easeInOut(duration: 0.18)) {
                isInspectorPresented.toggle()
            }
        } label: {
            Label(
                isInspectorPresented ? "Hide Inspector" : "Show Inspector",
                systemImage: "sidebar.right"
            )
        }
        .labelStyle(.iconOnly)
        .keyboardShortcut("i", modifiers: [.command, .option])
        .help(isInspectorPresented ? "Hide Inspector" : "Show Inspector")
        .accessibilityIdentifier("toggle-inspector")
    }
}
#endif

extension ContentView {
    #if os(iOS)
    var settingsToolbarButton: some View {
        Button {
            openSettings()
        } label: {
            Label("Settings", systemImage: "gearshape")
        }
        .labelStyle(.iconOnly)
        .accessibilityIdentifier("settings")
    }
    #endif

    var layoutMenu: some View {
        Menu {
            Section("Window layout") {
                ForEach(availableLayoutModes, id: \.self) { mode in
                    Button {
                        setLayoutMode(mode)
                    } label: {
                        if layout.mode == mode {
                            Label(mode.title, systemImage: "checkmark")
                        } else {
                            Label(mode.title, systemImage: mode.systemImage)
                        }
                    }
                }
            }
        } label: {
            Label(layout.mode.title, systemImage: layout.mode.systemImage)
        }
        .labelStyle(.iconOnly)
        .help("Change Window Layout")
        .accessibilityLabel("Window Layout")
        .accessibilityValue(layout.mode.title)
        .accessibilityHint(
            "Switches between freely arranged, automatically tiled, and full "
                + "window napplet display"
        )
        .accessibilityIdentifier("layout-mode-menu")
    }

    private var availableLayoutModes: [WorkbenchLayoutMode] {
        #if os(iOS)
        WorkbenchLayoutMode.allCases
        #else
        WorkbenchLayoutMode.allCases.filter { $0 != .fullWindow }
        #endif
    }

    @MainActor
    private func setLayoutMode(_ mode: WorkbenchLayoutMode) {
        mutateLayout { $0.setMode(mode) }
        if mode == .fullWindow {
            fullWindowRootID = layout.selectedWindow?.id
            fullWindowPath = []
        } else {
            fullWindowRootID = nil
            fullWindowPath = []
        }
    }

    @MainActor
    func exitFullWindow() {
        setLayoutMode(.freeform)
    }
}

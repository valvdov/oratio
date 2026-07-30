import SwiftUI

/// The three desktop themes, mirrored on iOS. As on desktop, the theme IS the
/// mode: Cream/Peach are light, Ember is dark — no separate mode switch.
enum OratioTheme: String, CaseIterable, Identifiable {
    case cream, peach, ember

    var id: String { rawValue }

    var label: String {
        switch self {
        case .cream: return "Cream"
        case .peach: return "Peach"
        case .ember: return "Ember"
        }
    }

    var accent: Color {
        switch self {
        case .cream: return Color(red: 0.773, green: 0.416, blue: 0.239) // #C56A3D
        case .peach: return Color(red: 0.886, green: 0.439, blue: 0.310) // #E2704F
        case .ember: return Color(red: 0.910, green: 0.639, blue: 0.239) // #E8A33D
        }
    }

    var colorScheme: ColorScheme {
        self == .ember ? .dark : .light
    }

    static var current: OratioTheme {
        OratioTheme(rawValue: SharedSettings.theme) ?? .ember
    }
}

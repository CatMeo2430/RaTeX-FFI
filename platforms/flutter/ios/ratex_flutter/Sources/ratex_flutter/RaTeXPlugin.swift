import Flutter
import UIKit

#if SWIFT_PACKAGE
import ratex_flutter_linker
#endif

/// Minimal Flutter plugin registration for ratex_flutter.
///
/// All rendering happens via Dart FFI (DynamicLibrary.process()) — the symbols
/// from libratex_ffi are linked into the app process through the xcframework.
/// No method channels or event channels are needed.
public class RaTeXPlugin: NSObject, FlutterPlugin {
    public static func register(with registrar: FlutterPluginRegistrar) {
        #if SWIFT_PACKAGE
        ratex_flutter_linker_anchor()
        #endif

        // FFI-only plugin: no channels to register.
    }
}

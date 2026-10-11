# Vendored GPUI

This directory contains the GPUI 0.2.2 source tree used by RosesVPN, including the platform crates needed for Windows, Linux, and macOS builds. It is kept in the RosesVPN repository so builds do not need to fetch the UI framework from another application repository.

GPUI is developed by Zed Industries. Its Apache License 2.0 text is preserved in `LICENSE-APACHE`. The workspace source snapshot identifier is `5ff26c7dedb1c8e27b69f89be13ce2744484874a`.

Keep these crates at one revision together: their manifests share dependencies and APIs across the GPUI workspace.

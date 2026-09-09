# Diagrams

Render `.puml` files to PNG/SVG with the preinstalled `plantuml-native` binary. No installs, no jars.

## Commands

```bash
# from repo root
plantuml-native -version          # check toolchain
plantuml-native -tpng docs/diagrams/hello-world.puml   # -> hello-world.png
plantuml-native -tsvg docs/diagrams/hello-world.puml   # -> hello-world.svg (optional)
plantuml-native -syntax docs/diagrams/hello-world.puml # syntax check only
```

## Notes

- Renderer: system `plantuml-native` 1.2025.4 (GraalVM native, GPLv2 distro). No `plantuml.jar` needed or vendored.
- One arrow per line: `A --> B --> C` on a single line fails (exit 200); split into two lines.
- Outputs (`.png`) sit next to their `.puml` source in this dir.

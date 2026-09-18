// The IntelliJ plugin: a client of the local endpoint the desktop app
// already serves (`apps/desktop/src-tauri/src/mcp.rs`).
//
// **Its own Gradle build, deliberately outside the Cargo workspace.** This is
// the only JVM code in the repository and nothing else depends on it; folding
// it into the Rust build would mean every `cargo test` needed a JDK.
//
// **Why a plugin at all, when a Gradle task already works.** The task needs
// the token in `gradle.properties`, which is a file, in a directory that is
// usually a git repository - that nearly leaked a token the first day the
// task existed. IntelliJ has a credential store backed by the OS keychain,
// and a plugin can use it. Everything else the plugin gives - picking an
// application from a list instead of pasting a UUID, logs in the IDE console -
// is convenience; this part is not.

plugins {
    kotlin("jvm") version "2.0.21"
    id("org.jetbrains.intellij.platform") version "2.19.0"
}

group = "dev.vibessh"
version = "0.1.0"

repositories {
    mavenCentral()
    intellijPlatform {
        defaultRepositories()
    }
}

dependencies {
    intellijPlatform {
        // Community edition: nothing here needs a paid IDE, and building
        // against the smaller platform keeps the plugin loadable in both.
        intellijIdeaCommunity("2024.2.5")
    }
    testImplementation(kotlin("test"))
}

kotlin {
    jvmToolchain(21)
}

intellijPlatform {
    pluginConfiguration {
        ideaVersion {
            // Open-ended on purpose. The plugin talks HTTP to localhost and
            // uses a handful of long-stable platform APIs; pinning an upper
            // bound would break it on every IDE release for no reason anybody
            // could point at.
            sinceBuild = "242"
            untilBuild = provider { null }
        }
    }
}

tasks.test {
    useJUnitPlatform()
}

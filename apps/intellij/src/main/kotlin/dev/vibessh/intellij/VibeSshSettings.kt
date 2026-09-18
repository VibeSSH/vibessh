package dev.vibessh.intellij

import com.intellij.credentialStore.CredentialAttributes
import com.intellij.credentialStore.Credentials
import com.intellij.credentialStore.generateServiceName
import com.intellij.ide.passwordSafe.PasswordSafe
import com.intellij.openapi.components.PersistentStateComponent
import com.intellij.openapi.components.Service
import com.intellij.openapi.components.State
import com.intellij.openapi.components.Storage
import com.intellij.openapi.components.service

/**
 * Where VibeSSH is and how to prove we may talk to it.
 *
 * **The token is the reason this plugin exists.** The Gradle task it
 * replaces needs the token in `gradle.properties` - a file, usually inside a
 * git repository, one `git commit -a` from a public remote. Here it goes to
 * `PasswordSafe`, which is the IDE's own credential store backed by the
 * operating system's keychain. It is never written to `.idea/`, never in the
 * project, never in anything that gets committed.
 *
 * Everything else here is not a secret and lives in ordinary settings: the
 * port, and which application to deploy to.
 */
@Service(Service.Level.APP)
@State(name = "VibeSshSettings", storages = [Storage("vibessh.xml")])
class VibeSshSettings : PersistentStateComponent<VibeSshSettings.State> {
    data class State(
        /** Matches the default in the desktop app's own settings. */
        var port: Int = 7422,
        /** The application id chosen last, so the tool window reopens on it. */
        var applicationId: String = "",
        /** Where inside the application's working directory a build lands. */
        var remoteDirectory: String = "plugins",
        /** Whether deploying should also restart the application. */
        var restartAfterDeploy: Boolean = true,
    )

    private var state = State()

    override fun getState(): State = state

    override fun loadState(state: State) {
        this.state = state
    }

    var port: Int
        get() = state.port
        set(value) {
            state.port = value
        }

    var applicationId: String
        get() = state.applicationId
        set(value) {
            state.applicationId = value
        }

    var remoteDirectory: String
        get() = state.remoteDirectory
        set(value) {
            state.remoteDirectory = value
        }

    var restartAfterDeploy: Boolean
        get() = state.restartAfterDeploy
        set(value) {
            state.restartAfterDeploy = value
        }

    /** `http://127.0.0.1:<port>` - loopback is not configurable, because it
     *  is not configurable on the other side either. */
    val baseUrl: String
        get() = "http://127.0.0.1:$port"

    /**
     * The token, read from the OS credential store on demand.
     *
     * Deliberately not cached in a field: a token that lived in memory for
     * the life of the IDE would survive being rotated in VibeSSH, and the
     * plugin would go on failing with a stale value nobody could see.
     */
    var token: String
        get() = PasswordSafe.instance.getPassword(credentialAttributes()) ?: ""
        set(value) {
            val attributes = credentialAttributes()
            if (value.isBlank()) {
                PasswordSafe.instance.set(attributes, null)
            } else {
                PasswordSafe.instance.set(attributes, Credentials(TOKEN_USER, value))
            }
        }

    companion object {
        private const val TOKEN_USER = "vibessh-local-endpoint"

        private fun credentialAttributes() =
            CredentialAttributes(generateServiceName("VibeSSH", TOKEN_USER))

        fun getInstance(): VibeSshSettings = service()
    }
}

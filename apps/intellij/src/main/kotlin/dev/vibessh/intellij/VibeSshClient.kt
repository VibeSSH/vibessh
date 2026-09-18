package dev.vibessh.intellij

import java.io.File
import java.net.HttpURLConnection
import java.net.URI
import java.nio.charset.StandardCharsets

/**
 * Talking to the endpoint the desktop app serves on loopback.
 *
 * Two shapes, because the endpoint has two. Tools are MCP - JSON-RPC over
 * `POST /mcp` - which is what an assistant uses and what answers "which
 * applications exist". Deploying is `POST /deploy` with the file as the raw
 * body, because a shaded jar is tens of megabytes and base64 inside a
 * JSON-RPC envelope would be a poor way to carry one.
 *
 * `HttpURLConnection` rather than a library: this makes four kinds of request
 * to localhost, and the platform already ships a client that does that. A
 * plugin that dragged in an HTTP stack would be a plugin with a dependency to
 * keep in step with every IDE release.
 */
class VibeSshClient(private val baseUrl: String, private val token: String) {

    /** What the endpoint said about itself, on a successful handshake. */
    data class ServerInfo(val name: String, val version: String)

    /** One row of `list_applications`, reduced to what the plugin shows. */
    data class Application(
        val id: String,
        val name: String,
        val status: String,
        val server: String?,
    ) {
        /** What the dropdown shows: the name, and where it runs. */
        override fun toString(): String = if (server == null) "$name (ten komputer)" else "$name ($server)"
    }

    class VibeSshException(message: String, cause: Throwable? = null) : RuntimeException(message, cause)

    /**
     * The MCP handshake, used by "Test connection".
     *
     * A successful `initialize` proves three things at once and is therefore
     * the whole of the test: the app is running, the endpoint is switched on,
     * and the token is right.
     */
    fun handshake(): ServerInfo {
        val response = rpc("initialize", "{}")
        val info = Json.objectAt(response, "result", "serverInfo")
            ?: throw VibeSshException("VibeSSH answered, but not like an MCP server - check the address")
        return ServerInfo(
            name = Json.stringAt(info, "name") ?: "vibessh",
            version = Json.stringAt(info, "version") ?: "?",
        )
    }

    fun listApplications(): List<Application> {
        val response = rpc("tools/call", """{"name":"list_applications","arguments":{}}""")
        val text = Json.stringAt(response, "result", "content", "0", "text")
            ?: throw VibeSshException("VibeSSH did not return an application list")
        return Json.applications(text)
    }

    /**
     * Sends a built file and, if asked, restarts what runs it.
     *
     * Returns whatever the endpoint said, which is already a readable
     * sentence - there is nothing this side could add to it.
     */
    fun deploy(applicationId: String, remotePath: String, file: File, restart: Boolean): String {
        val url = "$baseUrl/deploy?application=$applicationId&path=${encode(remotePath)}&restart=$restart"
        val connection = open(url, "application/octet-stream")
        connection.doOutput = true
        connection.setFixedLengthStreamingMode(file.length())
        connection.outputStream.use { out -> file.inputStream().use { it.copyTo(out) } }
        return read(connection)
    }

    private fun rpc(method: String, params: String): String {
        val connection = open("$baseUrl/mcp", "application/json")
        connection.doOutput = true
        val body = """{"jsonrpc":"2.0","id":1,"method":"$method","params":$params}"""
        connection.outputStream.use { it.write(body.toByteArray(StandardCharsets.UTF_8)) }
        val response = read(connection)
        // A JSON-RPC error is a 200 with an `error` member, so the HTTP status
        // having been fine is not the same as the call having worked.
        Json.stringAt(response, "error", "message")?.let { throw VibeSshException("VibeSSH: $it") }
        return response
    }

    private fun open(url: String, contentType: String): HttpURLConnection {
        val connection = URI(url).toURL().openConnection() as HttpURLConnection
        connection.requestMethod = "POST"
        connection.setRequestProperty("Authorization", "Bearer $token")
        connection.setRequestProperty("Content-Type", contentType)
        connection.connectTimeout = 5_000
        // Generous: a deploy uploads over SFTP to a real server and then
        // restarts it, which is not a five-second operation.
        connection.readTimeout = 120_000
        return connection
    }

    private fun read(connection: HttpURLConnection): String {
        val code = try {
            connection.responseCode
        } catch (err: Exception) {
            throw VibeSshException(
                "Nie udało się połączyć z VibeSSH pod $baseUrl - czy aplikacja działa i czy endpoint jest włączony?",
                err,
            )
        }
        if (code == 401) {
            throw VibeSshException("VibeSSH odrzucił token - skopiuj go ponownie z Ustawień")
        }
        if (code == 403) {
            throw VibeSshException("VibeSSH ma wyłączone „Zezwól na zmiany\" - włącz je w Ustawieniach")
        }
        val stream = if (code in 200..299) connection.inputStream else connection.errorStream
        val body = stream?.readBytes()?.toString(StandardCharsets.UTF_8).orEmpty()
        if (code !in 200..299) {
            throw VibeSshException("VibeSSH odpowiedział $code: ${body.ifBlank { "(bez treści)" }}")
        }
        return body
    }

    private fun encode(value: String): String =
        java.net.URLEncoder.encode(value, StandardCharsets.UTF_8).replace("+", "%20")
}

package dev.vibessh.intellij

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

/**
 * Reading what the endpoint actually sends.
 *
 * These are real response shapes, copied from what `mcp.rs` builds, not
 * invented ones - a test against a shape nobody sends proves nothing. The
 * other half of this file is about the endpoint sending something else
 * entirely: a plugin that threw into the IDE's error reporter because a
 * field moved would be worse than one that says it could not find it.
 */
class JsonTest {

    private val initialize = """
        {"jsonrpc":"2.0","id":1,"result":{
          "protocolVersion":"2024-11-05",
          "capabilities":{"tools":{}},
          "serverInfo":{"name":"vibessh","version":"0.1.0-beta.17"}
        }}
    """.trimIndent()

    @Test
    fun `reads the server info a handshake returns`() {
        assertEquals("vibessh", Json.stringAt(initialize, "result", "serverInfo", "name"))
        assertEquals("0.1.0-beta.17", Json.stringAt(initialize, "result", "serverInfo", "version"))
        assertTrue(Json.objectAt(initialize, "result", "serverInfo")!!.contains("vibessh"))
    }

    @Test
    fun `a json-rpc error is found where the client looks for it`() {
        val refused = """{"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"unknown tool: rm_rf"}}"""
        assertEquals("unknown tool: rm_rf", Json.stringAt(refused, "error", "message"))
    }

    @Test
    fun `an index walks into an array, which is how the tool result is nested`() {
        val call = """{"result":{"content":[{"type":"text","text":"[]"}]}}"""
        assertEquals("[]", Json.stringAt(call, "result", "content", "0", "text"))
    }

    @Test
    fun `reads the application list, including a local one`() {
        val applications = """
            [
              {"id":"c39141c9-dcdc-4c05-a432-6def05efaa4b","name":"royalmc-bedwars","runtime":"docker",
               "working_directory":"/home/container/royalmc-bedwars","status":"Running",
               "server_id":"1f0f6f2e-0000-4000-8000-000000000001"},
              {"id":"35404d26-6581-4cb9-9927-2a384af4458f","name":"test","runtime":"docker",
               "working_directory":"C:\\Users\\kompu\\apps\\test","status":"Stopped","server_id":null}
            ]
        """.trimIndent()

        val rows = Json.applications(applications)
        assertEquals(2, rows.size)
        assertEquals("royalmc-bedwars", rows[0].name)
        assertEquals("Running", rows[0].status)
        // A local application has no server, and the label has to say so
        // rather than leaving the reader to wonder where it runs.
        assertNull(rows[1].server)
        assertTrue(rows[1].toString().contains("ten komputer"))
    }

    /// A row with no id is one that would fail the moment somebody picked it,
    /// because the id is the only part the deploy call uses.
    @Test
    fun `a row without an id is left out rather than offered`() {
        val applications = """[{"name":"broken"},{"id":"abc","name":"fine"}]"""
        val rows = Json.applications(applications)
        assertEquals(1, rows.size)
        assertEquals("abc", rows[0].id)
    }

    /// Everything below is the endpoint sending something this plugin did not
    /// expect. None of it may throw.
    @Test
    fun `nothing here throws on a shape it did not expect`() {
        assertNull(Json.stringAt("not json at all", "result"))
        assertNull(Json.stringAt("", "result"))
        assertNull(Json.stringAt("""{"result":null}""", "result", "serverInfo"))
        assertNull(Json.stringAt("""{"result":{}}""", "result", "serverInfo", "name"))
        assertNull(Json.stringAt("""{"result":[]}""", "result", "9", "text"))
        assertNull(Json.stringAt("""{"result":{"serverInfo":{}}}""", "result", "serverInfo", "name"))
        // A number where a string was expected is not a string.
        assertNull(Json.stringAt("""{"result":{"name":{"nested":1}}}""", "result", "name"))
        assertEquals(emptyList(), Json.applications("{}"))
        assertEquals(emptyList(), Json.applications("nonsense"))
        assertEquals(emptyList(), Json.applications("""[1,2,3]"""))
    }
}

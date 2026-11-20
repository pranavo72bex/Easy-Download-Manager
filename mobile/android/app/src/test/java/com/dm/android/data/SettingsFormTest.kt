// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

package com.dm.android.data

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class SettingsFormTest {
    private val json = """
        {"watch_clipboard":false,"max_connections":8,"ratio":1.5,"download_dir":"/sdcard/Download/dm",
         "category_folders":["Video","Music"],"proxy":null,
         "slow_hours":{"enabled":true,"limit_kb":64}}
    """.trimIndent()

    @Test
    fun eachValueIsALineOfItsKind() {
        val sections = SettingsForm.read(json)
        val top = sections.first().fields.associateBy { it.key }
        assertEquals(FieldKind.SWITCH, top.getValue("watch_clipboard").kind)
        assertEquals(FieldKind.WHOLE_NUMBER, top.getValue("max_connections").kind)
        assertEquals(FieldKind.NUMBER, top.getValue("ratio").kind)
        assertEquals(FieldKind.TEXT, top.getValue("download_dir").kind)
        assertEquals("Watch clipboard", top.getValue("watch_clipboard").label)
        assertFalse("a list is not shown", top.containsKey("category_folders"))
        assertFalse("an empty value is not shown", top.containsKey("proxy"))
        val slow = sections[1]
        assertEquals("Slow hours", slow.title)
        assertEquals(listOf("slow_hours/enabled", "slow_hours/limit_kb"), slow.fields.map { it.key })
        assertEquals("Limit kb", slow.fields[1].label)
    }

    @Test
    fun writingKeepsWhatIsNotShownAndPutsInWhatWasEdited() {
        val fields = SettingsForm.read(json).flatMap { it.fields }.map {
            when (it.key) {
                "watch_clipboard" -> it.copy(value = "true")
                "max_connections" -> it.copy(value = " 16 ")
                "slow_hours/limit_kb" -> it.copy(value = "128")
                else -> it
            }
        }
        val written = Json.parseToJsonElement(SettingsForm.write(json, fields)).jsonObject
        assertEquals("true", written.getValue("watch_clipboard").jsonPrimitive.content)
        assertEquals("16", written.getValue("max_connections").jsonPrimitive.content)
        assertEquals("128", (written.getValue("slow_hours") as JsonObject).getValue("limit_kb").jsonPrimitive.content)
        assertEquals(2, (written.getValue("category_folders") as kotlinx.serialization.json.JsonArray).size)
        assertTrue(written.containsKey("proxy"))
    }

    @Test(expected = NotANumber::class)
    fun aNumberFieldMustHoldANumber() {
        val fields = SettingsForm.read(json).flatMap { it.fields }.map {
            if (it.key == "max_connections") it.copy(value = "many") else it
        }
        SettingsForm.write(json, fields)
    }
}

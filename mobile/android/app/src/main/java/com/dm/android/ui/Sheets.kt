// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

package com.dm.android.ui

import android.content.Context
import android.content.Intent
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.dm.android.R
import com.dm.android.data.Format
import com.dm.android.data.Links
import uniffi.dm_android.Row
import uniffi.dm_android.RowState

/** A download's details (gaps row N7): every field of its row, and what can be done to it. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun DetailsSheet(row: Row, model: DownloadsViewModel) {
    ModalBottomSheet(onDismissRequest = { model.select(null) }) {
        Column(
            Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal = 20.dp, vertical = 8.dp),
        ) {
            Text(row.name, style = MaterialTheme.typography.titleLarge)
            Spacer(Modifier.height(12.dp))
            DetailLines(row)
            Spacer(Modifier.height(16.dp))
            DetailButtons(row, model)
            Spacer(Modifier.height(24.dp))
        }
    }
}

@Composable
private fun DetailLines(row: Row) {
    val unknown = stringResource(R.string.unknown)
    Detail(R.string.detail_status, stateWord(row.state))
    row.failure?.let { Detail(R.string.detail_failure, it) }
    Detail(R.string.detail_address, row.url)
    Detail(R.string.detail_size, row.total?.let { (if (row.estimated) "≈" else "") + Format.bytes(it.toLong()) } ?: unknown)
    Detail(R.string.detail_downloaded, Format.bytes(row.downloaded.toLong()))
    if (row.state == RowState.RUNNING) {
        Detail(R.string.detail_speed, Format.speed(row.speed))
        Detail(R.string.detail_eta, row.etaSeconds?.let { Format.eta(it.toLong()) } ?: unknown)
    }
    if (row.path.isNotEmpty()) Detail(R.string.detail_saved_to, row.path)
    row.category?.let { Detail(R.string.detail_category, Format.label(it)) }
    row.swarm?.let { swarm ->
        Detail(
            R.string.detail_peers,
            stringResource(
                R.string.peers_line,
                swarm.connected.toInt(),
                swarm.known.toInt(),
                Format.bytes(swarm.uploaded.toLong()),
                Format.speed(swarm.uploadSpeed),
            ),
        )
    }
}

@Composable
private fun Detail(label: Int, value: String) {
    Column(Modifier.fillMaxWidth().padding(vertical = 4.dp)) {
        Text(stringResource(label), style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.primary)
        Text(value, style = MaterialTheme.typography.bodyMedium)
    }
}

/** The buttons a row's state allows, then Copy link, Share link and Remove from list. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun DetailButtons(row: Row, model: DownloadsViewModel) {
    val clipboard = LocalClipboardManager.current
    val context = LocalContext.current
    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        when (row.state) {
            RowState.RUNNING, RowState.QUEUED -> {
                Action(R.string.action_stop) { model.pause(row.id) }
                Action(R.string.action_cancel) { model.cancel(row.id) }
            }
            RowState.PAUSED -> {
                Action(R.string.action_resume) { model.resume(row.id) }
                Action(R.string.action_cancel) { model.cancel(row.id) }
            }
            RowState.FAILED -> Action(R.string.action_retry) { model.retry(row.id) }
            RowState.COMPLETED -> {
                Action(R.string.action_open) { model.open(row.path) }
                Action(R.string.action_open_folder) { model.reveal(row.path) }
            }
            RowState.CANCELLED -> Unit
        }
        Action(R.string.action_copy_link) {
            clipboard.setText(AnnotatedString(row.url))
            model.tell(UiMessage.Res(R.string.link_copied))
        }
        Action(R.string.action_share_link) { shareLink(context, row.url) }
        Action(R.string.action_remove) { model.remove(row.id) }
    }
}

@Composable
private fun Action(words: Int, onClick: () -> Unit) {
    OutlinedButton(onClick = onClick) { Text(stringResource(words)) }
}

private fun shareLink(context: Context, url: String) {
    val send = Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, url)
    context.startActivity(Intent.createChooser(send, null).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
}

/**
 * Add URL (gaps row N5): the links typed, pasted or handed over, the folder they go to, and
 * Download. What is added is what the engine finds in the text; the list here is a preview.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AddSheet(draft: String, model: DownloadsViewModel) {
    val clipboard = LocalClipboardManager.current
    val folder by model.downloadDir.collectAsStateWithLifecycle()
    val found = Links.found(draft)
    ModalBottomSheet(onDismissRequest = model::closeAdd) {
        Column(Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal = 20.dp, vertical = 8.dp)) {
            Text(stringResource(R.string.action_add), style = MaterialTheme.typography.titleLarge)
            Spacer(Modifier.height(12.dp))
            OutlinedTextField(
                value = draft,
                onValueChange = model::editAdd,
                label = { Text(stringResource(R.string.add_hint)) },
                minLines = 3,
                modifier = Modifier.fillMaxWidth().testTag("add-text"),
            )
            TextButton(onClick = {
                val pasted = clipboard.getText()?.text.orEmpty()
                if (pasted.isNotBlank()) model.editAdd(if (draft.isBlank()) pasted else "$draft\n$pasted")
            }) { Text(stringResource(R.string.add_paste)) }
            FoundLinks(found)
            folder?.let {
                Spacer(Modifier.height(8.dp))
                Text(stringResource(R.string.add_save_to, it), style = MaterialTheme.typography.bodySmall)
            }
            Spacer(Modifier.height(12.dp))
            Button(onClick = model::submitAdd, enabled = found.isNotEmpty(), modifier = Modifier.fillMaxWidth()) {
                Text(stringResource(R.string.add_start))
            }
            Spacer(Modifier.height(24.dp))
        }
    }
}

@Composable
private fun FoundLinks(found: List<String>) {
    if (found.isEmpty()) {
        Text(stringResource(R.string.add_none_found), style = MaterialTheme.typography.bodySmall)
        return
    }
    val context = LocalContext.current
    Text(
        context.resources.getQuantityString(R.plurals.add_found, found.size, found.size),
        style = MaterialTheme.typography.labelMedium,
    )
    found.forEach { link ->
        Text(link, maxLines = 1, overflow = TextOverflow.Ellipsis, style = MaterialTheme.typography.bodySmall)
    }
}

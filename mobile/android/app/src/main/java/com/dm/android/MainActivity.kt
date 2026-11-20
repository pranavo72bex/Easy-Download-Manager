// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

package com.dm.android

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.viewModels
import androidx.core.content.ContextCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.repeatOnLifecycle
import com.dm.android.data.active
import com.dm.android.engine.EngineHolder
import com.dm.android.engine.EngineSource
import com.dm.android.service.DownloadService
import com.dm.android.share.Incoming
import com.dm.android.share.IncomingLinks
import com.dm.android.ui.DmScreens
import com.dm.android.ui.DmTheme
import com.dm.android.ui.DownloadsViewModel
import com.dm.android.ui.UiMessage
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.filter
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * dm's one window. A link shared to dm, a magnet or a `.torrent` opened with it reaches the
 * Add sheet here (gaps row N5); the list, details and Options are [DmScreens].
 */
class MainActivity : ComponentActivity() {
    private val model: DownloadsViewModel by viewModels {
        DownloadsViewModel.Factory(EngineSource(applicationContext))
    }

    private val askPermissions =
        registerForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent { DmTheme { DmScreens(model) } }
        askWhatIsNeeded()
        keepServiceWhileActive()
        if (savedInstanceState == null) handOver(intent)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        handOver(intent)
    }

    override fun onStart() {
        super.onStart()
        EngineHolder.windowShown = true
    }

    override fun onStop() {
        EngineHolder.windowShown = false
        super.onStop()
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (hasFocus) model.checkClipboard()
    }

    /** What another app handed dm, into the Add sheet. */
    private fun handOver(intent: Intent?) {
        lifecycleScope.launch {
            when (val incoming = withContext(Dispatchers.IO) { IncomingLinks.from(applicationContext, intent) }) {
                is Incoming.Text -> model.openAdd(incoming.text)
                is Incoming.Unreadable -> model.tell(UiMessage.Res(R.string.torrent_unreadable, incoming.why))
                null -> Unit
            }
        }
    }

    /** The service is started whenever something is on its way, a restored queue's too. */
    private fun keepServiceWhileActive() {
        lifecycleScope.launch {
            repeatOnLifecycle(Lifecycle.State.STARTED) {
                model.rows
                    .map { rows -> rows.any { it.active() } }
                    .distinctUntilChanged()
                    .filter { it }
                    .collect { DownloadService.start(applicationContext) }
            }
        }
    }

    /** Notifications (Android 13 and up) and, before Android 10, the Download folder. */
    private fun askWhatIsNeeded() {
        val wanted = buildList {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) add(Manifest.permission.POST_NOTIFICATIONS)
            if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) add(Manifest.permission.WRITE_EXTERNAL_STORAGE)
        }.filter { ContextCompat.checkSelfPermission(this, it) != PackageManager.PERMISSION_GRANTED }
        if (wanted.isNotEmpty()) askPermissions.launch(wanted.toTypedArray())
    }
}

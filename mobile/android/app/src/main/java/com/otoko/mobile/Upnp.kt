package com.otoko.mobile

import android.content.Context
import android.net.wifi.WifiManager
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.io.IOException
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.HttpURLConnection
import java.net.InetAddress
import java.net.SocketTimeoutException
import java.net.URL

/** Un lecteur réseau UPnP/DLNA (Naim Mu-so…) trouvé sur le Wi-Fi */
data class UpnpDevice(
    val name: String,
    val location: String,
    val udn: String,
    val avTransport: String,
    val renderingControl: String?,
)

/** Recherche (SSDP) et commandes (SOAP) des lecteurs UPnP. Tout se passe hors du thread principal. */
object Upnp {
    const val AV_TRANSPORT = "urn:schemas-upnp-org:service:AVTransport:1"
    const val RENDERING = "urn:schemas-upnp-org:service:RenderingControl:1"

    /** Lecteurs qui répondent en ~3 s sur le Wi-Fi */
    suspend fun discover(context: Context, timeoutMs: Int = 3000): List<UpnpDevice> = withContext(Dispatchers.IO) {
        val wifi = context.applicationContext.getSystemService(WifiManager::class.java)
        val lock = wifi?.createMulticastLock("otoko-upnp")?.apply { setReferenceCounted(false); acquire() }
        val locations = linkedSetOf<String>()
        try {
            DatagramSocket().use { socket ->
                socket.soTimeout = 300
                val msg = ("M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nMAN: \"ssdp:discover\"\r\nMX: 2\r\n" +
                    "ST: urn:schemas-upnp-org:device:MediaRenderer:1\r\n\r\n").toByteArray()
                val group = InetAddress.getByName("239.255.255.250")
                repeat(2) { socket.send(DatagramPacket(msg, msg.size, group, 1900)) }
                val buf = ByteArray(8192)
                val end = System.currentTimeMillis() + timeoutMs
                while (System.currentTimeMillis() < end) {
                    val p = DatagramPacket(buf, buf.size)
                    try { socket.receive(p) } catch (e: SocketTimeoutException) { continue }
                    val text = String(p.data, 0, p.length, Charsets.ISO_8859_1)
                    Regex("(?im)^location:\\s*(\\S+)").find(text)?.groupValues?.get(1)?.let { locations += it }
                }
            }
        } finally {
            lock?.release()
        }
        locations.mapNotNull { runCatching { describe(it) }.getOrNull() }.distinctBy { it.udn }
    }

    /** Lit la description XML d'un lecteur (nom, adresses de commande) */
    fun describe(location: String): UpnpDevice? {
        val xml = httpGet(location)
        fun tag(src: String, t: String) = Regex("<(?:\\w+:)?$t>\\s*(.*?)\\s*</(?:\\w+:)?$t>", RegexOption.DOT_MATCHES_ALL)
            .find(src)?.groupValues?.get(1)
        val base = tag(xml, "URLBase")?.takeIf { it.startsWith("http") } ?: location
        val services = Regex("<(?:\\w+:)?service>(.*?)</(?:\\w+:)?service>", RegexOption.DOT_MATCHES_ALL)
            .findAll(xml).map { it.groupValues[1] }
            .associate { (tag(it, "serviceType") ?: "") to (tag(it, "controlURL") ?: "") }
        val av = services.entries.firstOrNull { it.key.startsWith("urn:schemas-upnp-org:service:AVTransport") }?.value
            ?: return null
        val rc = services.entries.firstOrNull { it.key.startsWith("urn:schemas-upnp-org:service:RenderingControl") }?.value
        return UpnpDevice(
            name = unescape(tag(xml, "friendlyName") ?: "Lecteur réseau"),
            location = location,
            udn = tag(xml, "UDN") ?: location,
            avTransport = URL(URL(base), av).toString(),
            renderingControl = rc?.let { URL(URL(base), it).toString() },
        )
    }

    /** Appel SOAP ; renvoie la réponse XML ou lève IOException */
    fun soap(controlUrl: String, service: String, action: String, args: List<Pair<String, String>>, timeout: Int = 4000): String {
        val body = buildString {
            append("<?xml version=\"1.0\" encoding=\"utf-8\"?>")
            append("<s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" ")
            append("s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\"><s:Body>")
            append("<u:$action xmlns:u=\"$service\">")
            args.forEach { (k, v) -> append("<$k>").append(escape(v)).append("</$k>") }
            append("</u:$action></s:Body></s:Envelope>")
        }.toByteArray(Charsets.UTF_8)
        val c = URL(controlUrl).openConnection() as HttpURLConnection
        c.connectTimeout = timeout
        c.readTimeout = timeout
        c.requestMethod = "POST"
        c.doOutput = true
        c.setRequestProperty("Content-Type", "text/xml; charset=\"utf-8\"")
        c.setRequestProperty("SOAPACTION", "\"$service#$action\"")
        c.setFixedLengthStreamingMode(body.size)
        try {
            c.outputStream.use { it.write(body) }
            val code = c.responseCode
            val text = (if (code < 400) c.inputStream else c.errorStream)?.bufferedReader()?.readText() ?: ""
            if (code >= 400) throw IOException("$action refusé ($code)")
            return text
        } finally {
            c.disconnect()
        }
    }

    fun value(xml: String, tag: String): String? =
        Regex("<(?:\\w+:)?$tag>(.*?)</(?:\\w+:)?$tag>", RegexOption.DOT_MATCHES_ALL).find(xml)?.groupValues?.get(1)

    fun httpGet(url: String, timeout: Int = 3000): String {
        val c = URL(url).openConnection() as HttpURLConnection
        c.connectTimeout = timeout
        c.readTimeout = timeout
        try {
            if (c.responseCode != 200) throw IOException("Réponse ${c.responseCode}")
            return c.inputStream.bufferedReader().readText()
        } finally {
            c.disconnect()
        }
    }

    fun escape(s: String) = s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;").replace("\"", "&quot;")
    private fun unescape(s: String) = s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&amp;", "&")

    /** 125 s → « 0:02:05 » (format UPnP) */
    fun hms(ms: Long): String {
        val s = (ms / 1000).coerceAtLeast(0)
        return "%d:%02d:%02d".format(s / 3600, (s / 60) % 60, s % 60)
    }

    /** « 0:02:05 » ou « 0:02:05.500 » → millisecondes (null si NOT_IMPLEMENTED…) */
    fun parseHms(s: String?): Long? {
        val parts = s?.trim()?.split(":") ?: return null
        if (parts.size != 3) return null
        val h = parts[0].toLongOrNull() ?: return null
        val m = parts[1].toLongOrNull() ?: return null
        val sec = parts[2].toDoubleOrNull() ?: return null
        return ((h * 3600 + m * 60) * 1000 + sec * 1000).toLong()
    }
}

using System;
using FishNet.Managing;
using FishNet.Managing.Transporting;
using FishNet.Transporting.FishyWebRTC;
using FishNet.Transporting;
using FishNet.Connection;
using UnityEngine;

/// Bootstrap for the FishNet NetworkTransform demo with FishyWebRTC.
///
/// - Resolves the listen/connect port from `$PORT` or `-port <n>` (default 7770) so it
///   works as the container-runner's child (which sets $PORT).
/// - Sets that port on FishyWebRTC, which is wired up by ProjectBootstrap.
/// - As a dedicated server / -batchmode build: starts the server socket.
/// - As a native `-client`, reads the signaling base URL from `-url <url>`.
/// - WebGL remains compatible and reads the URL from `?server=<url>`.
[DefaultExecutionOrder(-10000)]
public class ServerBootstrap : MonoBehaviour
{
    private const string DefaultIceUrls =
        "stun:stun.l.google.com:19302,stun:stun.cloudflare.com:3478";

    private NetworkManager _nm;

    private void Awake()
    {
        // WebRTC negotiation and data channels must keep progressing when the native
        // client loses focus (for example, while inspecting Cloud logs).
        Application.runInBackground = true;
        _nm = GetComponent<NetworkManager>();
        if (_nm == null)
            _nm = FindAnyObjectByType<NetworkManager>();
    }

    private void Start()
    {
        ushort port = ResolvePort(7770);
        Transport transport = _nm.TransportManager.Transport;
        transport.SetPort(port);
        FishyWebRTC webRtc = transport as FishyWebRTC;
        if (webRtc == null)
        {
            Debug.LogError($"[bootstrap] expected FishyWebRTC, got {transport.GetType().Name}");
            return;
        }
        ConfigureIceServers(webRtc);

        bool isClient = Application.platform == RuntimePlatform.WebGLPlayer || HasArg("-client");
        if (isClient)
        {
            string url = ArgValue("-url") ?? QueryValue("server");
            if (string.IsNullOrWhiteSpace(url) || !Uri.TryCreate(url, UriKind.Absolute, out Uri uri))
            {
                Debug.LogError("[bootstrap] WebRTC client requires -url https://api.rivet.dev/gateway/<actor_id>@<token>/request/");
                return;
            }

            string address = uri.Host;
            string path = string.IsNullOrEmpty(uri.AbsolutePath) ? "/" : uri.AbsolutePath;
            bool useHttps = uri.Scheme == Uri.UriSchemeHttps;
            port = (ushort)(uri.IsDefaultPort ? (useHttps ? 443 : 80) : uri.Port);
            transport.SetPort(port);
            transport.SetClientAddress(address);
            webRtc.SetHTTPS(useHttps);
            webRtc.SetNoClientPort(uri.IsDefaultPort);
            webRtc.SetSignalingPath(path);
            _nm.ClientManager.OnClientConnectionState += OnClientState;
            // Keep RTT logging enabled in headless E2E runs; only the visual HUD is
            // limited to the windowed client.
            _nm.TimeManager.OnRoundTripTimeUpdated += OnRttUpdated;
            _hudEnabled = !Application.isBatchMode;
            Debug.Log($"[bootstrap] starting WebRTC CLIENT -> {uri.GetLeftPart(UriPartial.Authority)}{RedactToken(path)}");
            _nm.ClientManager.StartConnection();
        }
        else
        {
            _nm.ServerManager.OnServerConnectionState += OnServerState;
            _nm.ServerManager.OnRemoteConnectionState += OnRemoteState;
            Debug.Log($"[bootstrap] starting SERVER on port {port}");
            _nm.ServerManager.StartConnection();
        }
    }

    private static string QueryValue(string name)
    {
        if (string.IsNullOrWhiteSpace(Application.absoluteURL))
            return null;

        Uri uri = new Uri(Application.absoluteURL);
        string query = uri.Query.TrimStart('?');
        foreach (string pair in query.Split('&'))
        {
            string[] parts = pair.Split(new[] { '=' }, 2);
            if (parts.Length == 2 && Uri.UnescapeDataString(parts[0]) == name)
                return Uri.UnescapeDataString(parts[1]);
        }
        return null;
    }

    // --- latency HUD (client only) -------------------------------------------------
    // FishNet's RoundTripTime "includes latency from the tick rate" (see TimeManager),
    // so it is NOT pure network RTT. We show the raw value plus the tick-rate component
    // so a bad reading can be attributed to the wire vs. the tick.
    private readonly System.Collections.Generic.List<long> _rttSamples = new();
    private long _rttLast;
    private int _rttLogCounter;
    private bool _hudEnabled;
    private GUIStyle _hudStyle;

    private void OnRttUpdated(long rtt)
    {
        _rttLast = rtt;
        _rttSamples.Add(rtt);
        if (_rttSamples.Count > 600)
            _rttSamples.RemoveAt(0);
        _rttLogCounter++;
        if (_rttLogCounter == 1 || _rttLogCounter % 60 == 0)
            Debug.Log($"[bootstrap] WebRTC RTT sample: {rtt} ms");
    }

    private void OnGUI()
    {
        if (!_hudEnabled || _nm == null)
            return;

        _hudStyle ??= new GUIStyle(GUI.skin.label)
        {
            fontSize = 14,
            normal = { textColor = Color.white },
        };

        long avg = 0, min = 0, max = 0, p95 = 0;
        int n = _rttSamples.Count;
        if (n > 0)
        {
            var sorted = new System.Collections.Generic.List<long>(_rttSamples);
            sorted.Sort();
            min = sorted[0];
            max = sorted[n - 1];
            p95 = sorted[Mathf.Min(n - 1, Mathf.FloorToInt(n * 0.95f))];
            long sum = 0;
            foreach (long v in sorted) sum += v;
            avg = sum / n;
        }

        ushort tickRate = _nm.TimeManager.TickRate;
        double tickMs = tickRate > 0 ? 1000d / tickRate : 0d;

        var rect = new Rect(10, 10, 460, 110);
        GUI.Box(rect, GUIContent.none);
        GUILayout.BeginArea(new Rect(18, 16, 450, 100));
        GUILayout.Label($"RTT {_rttLast} ms   (avg {avg}  min {min}  p95 {p95}  max {max}, n={n})", _hudStyle);
        GUILayout.Label($"tick {tickRate} Hz -> ~{tickMs:F0} ms of that RTT is tick quantization", _hudStyle);
        GUILayout.Label($"one-way (half RTT) ~{_nm.TimeManager.HalfRoundTripTime} ms", _hudStyle);
        GUILayout.EndArea();
    }

    private void OnClientState(ClientConnectionStateArgs args)
    {
        Debug.Log($"[bootstrap] client connection state: {args.ConnectionState}");
        if (args.ConnectionState == LocalConnectionState.Started)
            Debug.Log("[bootstrap] CLIENT CONNECTED");
    }

    private void OnServerState(ServerConnectionStateArgs args)
    {
        Debug.Log($"[bootstrap] server connection state: {args.ConnectionState}");
        if (args.ConnectionState == LocalConnectionState.Started)
            Debug.Log("[bootstrap] SERVER STARTED");
    }

    private void OnRemoteState(NetworkConnection conn, RemoteConnectionStateArgs args)
    {
        Debug.Log($"[bootstrap] remote client {conn.ClientId} state: {args.ConnectionState}");
        if (args.ConnectionState == RemoteConnectionState.Started)
            Debug.Log($"[bootstrap] SERVER ACCEPTED CLIENT {conn.ClientId}");
    }

    private static ushort ResolvePort(ushort dflt)
    {
        string env = Environment.GetEnvironmentVariable("PORT");
        if (ushort.TryParse(env, out ushort p)) return p;
        string arg = ArgValue("-port");
        if (arg != null && ushort.TryParse(arg, out ushort p2)) return p2;
        return dflt;
    }

    private static void ConfigureIceServers(FishyWebRTC webRtc)
    {
        string urls = ArgValue("-ice-url") ?? Environment.GetEnvironmentVariable("WEBRTC_ICE_URL");
        if (string.IsNullOrWhiteSpace(urls))
            urls = DefaultIceUrls;

        string username =
            ArgValue("-ice-username") ?? Environment.GetEnvironmentVariable("WEBRTC_ICE_USERNAME");
        string credential =
            ArgValue("-ice-credential") ?? Environment.GetEnvironmentVariable("WEBRTC_ICE_CREDENTIAL");

        int count = webRtc.SetIceServers(urls, username, credential);
        bool hasTurn = false;
        foreach (string rawUrl in urls.Split(','))
        {
            string url = rawUrl.Trim();
            if (url.StartsWith("turn:", StringComparison.OrdinalIgnoreCase) ||
                url.StartsWith("turns:", StringComparison.OrdinalIgnoreCase))
            {
                hasTurn = true;
                break;
            }
        }

        Debug.Log(
            $"[bootstrap] configured {count} runtime ICE server(s): {urls}; " +
            $"TURN {(hasTurn ? "enabled" : "disabled (STUN-only native P2P)")}");
    }

    private static string RedactToken(string path)
    {
        int at = path.IndexOf('@');
        if (at < 0)
            return path;

        int slash = path.IndexOf('/', at);
        if (slash < 0)
            slash = path.Length;
        return path.Substring(0, at + 1) + "<token>" + path.Substring(slash);
    }

    private static bool HasArg(string name)
    {
        foreach (string a in Environment.GetCommandLineArgs())
            if (a == name) return true;
        return false;
    }

    private static string ArgValue(string name)
    {
        string[] args = Environment.GetCommandLineArgs();
        for (int i = 0; i < args.Length - 1; i++)
            if (args[i] == name) return args[i + 1];
        return null;
    }
}

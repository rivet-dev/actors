using FishNet.Editing.NetworkProfiler;
using FishNet.Managing;
using FishNet.Managing.Statistic;
using FishNet.Managing.Timing;
using GameKit.Dependencies.Utilities.Types;
using UnityEngine;

namespace FishNet.Component.Utility
{
    /// <summary>
    /// Add to any object to display current ping(round trip time).
    /// </summary>
    [AddComponentMenu("FishNet/Component/BandwidthDisplay")]
    public class BandwidthDisplay : MonoBehaviour
    {
        #region Types.
        private enum Corner
        {
            TopLeft,
            TopRight,
            BottomLeft,
            BottomRight
        }

        public class InOutAverage
        {
            private RingBuffer<ulong> _in;
            private RingBuffer<ulong> _out;

            public InOutAverage(int ticks)
            {
                _in = new(ticks);
                _out = new(ticks);
            }

            public void AddIn(ulong value) => _in.Add(value);
            public void AddOut(ulong value) => _out.Add(value);

            public float GetAverage(bool inBuffer)
            {
                RingBuffer<ulong> buffer = GetBuffer(inBuffer);

                int bufferCount = buffer.Count;
                if (bufferCount == 0)
                    return 0;
                
                ulong total = GetTotal(inBuffer);
                return (float)total / bufferCount;
            }

            public ulong GetTotal(bool inBuffer)
            {
                RingBuffer<ulong> buffer = GetBuffer(inBuffer);
                
                ulong total = 0;
                foreach (ulong v in buffer)
                    total += v;
                
                return total;
            }
            
            private RingBuffer<ulong> GetBuffer(bool inBuffer) => inBuffer ? _in : _out;
            
            public void ResetState()
            {
                _in.Clear();
                _out.Clear();
            }

            public void InitializeState(int capacity)
            {
                _in.Initialize(capacity);
                _out.Initialize(capacity);
            }
        }
        #endregion

        #region Public.
        #if UNITY_EDITOR || !UNITY_SERVER
        /// <summary>
        /// Averages for client.
        /// </summary>
        public InOutAverage ClientAverages { get; private set; }
        /// <summary>
        /// Averages for server.
        /// </summary>
        public InOutAverage ServerAverages { get; private set; }
        #endif
        #endregion

        #region Serialized.
        [Header("Misc")]
        /// <summary>
        /// True to operate while in release. This may cause allocations and impact performance.
        /// </summary>
        [Tooltip("True to operate while in release. This may cause allocations and impact performance.")]
        [SerializeField]
        private bool _runInRelease;
        [Header("Timing")]
        /// <summary>
        /// Number of seconds used to gather data per second. Lower values will show more up to date usage per second while higher values provide a better over-all estimate.
        /// </summary>
        [Tooltip("Number of seconds used to gather data per second. Lower values will show more up to date usage per second while higher values provide a better over-all estimate.")]
        [SerializeField]
        [Range(1, byte.MaxValue)]
        private byte _secondsAveraged = 1;
        /// <summary>
        /// How often to update displayed text.
        /// </summary>
        [Tooltip("How often to update displayed text.")]
        [Range(0f, 10f)]
        [SerializeField]
        private float _updateInterval = 1f;
        [Header("Appearance")]
        /// <summary>
        /// Color for text.
        /// </summary>
        [Tooltip("Color for text.")]
        [SerializeField]
        private Color _color = Color.white;
        /// <summary>
        /// Which corner to display network statistics in.
        /// </summary>
        [Tooltip("Which corner to display network statistics in.")]
        [SerializeField]
        private Corner _placement = Corner.TopRight;
        /// <summary>
        /// rue to show outgoing data bytes.
        /// </summary>
        [Tooltip("True to show outgoing data bytes.")]
        [SerializeField]
        private bool _showOutgoing = true;

        /// <summary>
        /// Sets ShowOutgoing value.
        /// </summary>
        /// <param name = "value"></param>
        public void SetShowOutgoing(bool value) => _showOutgoing = value;

        /// <summary>
        /// True to show incoming data bytes.
        /// </summary>
        [Tooltip("True to show incoming data bytes.")]
        [SerializeField]
        private bool _showIncoming = true;

        /// <summary>
        /// Sets ShowIncoming value.
        /// </summary>
        /// <param name = "value"></param>
        public void SetShowIncoming(bool value) => _showIncoming = value;
        #endregion

        #if UNITY_EDITOR || !UNITY_SERVER

        #region Private.
        /// <summary>
        /// Style for drawn ping.
        /// </summary>
        private readonly GUIStyle _style = new();
        /// <summary>
        /// Text to show for client in/out data.
        /// </summary>
        private string _clientText;
        /// <summary>
        /// Text to show for server in/out data.
        /// </summary>
        private string _serverText;
        /// <summary>
        /// First found NetworkTrafficStatistics.
        /// </summary>
        private NetworkTrafficStatistics _networkTrafficStatistics;
        /// <summary>
        /// Next time the server text can be updated.
        /// </summary>
        private float _nextServerTextUpdateTime;
        /// <summary>
        /// Next time the server text can be updated.
        /// </summary>
        private float _nextClientTextUpdateTime;
        /// <summary>
        /// True if component is initialized.
        /// </summary>
        private bool _initialized;
        #endregion

        private void Start()
        {
            // Requires a UI, so exit if server build.
            #if UNITY_SERVER
            return;
            #endif
            // If release build, check if able to run in release.
            #if !DEVELOPMENT_BUILD && !UNITY_EDITOR
            if (!_runInRelease)
                return;
            #endif

            // Not enabled.
            if (!InstanceFinder.NetworkManager.StatisticsManager.TryGetNetworkTrafficStatistics(out _networkTrafficStatistics))
                return;

            if (!_networkTrafficStatistics.UpdateClient && !_networkTrafficStatistics.UpdateServer)
            {
                Debug.LogWarning($"StatisticsManager.NetworkTraffic is not updating for client nor server. To see results ensure your NetworkManager has a StatisticsManager component added with the NetworkTraffic values configured.");
                return;
            }

            SetSecondsAveraged(_secondsAveraged);

            _networkTrafficStatistics.OnNetworkTraffic += NetworkTrafficStatistics_OnNetworkTraffic;

            _initialized = true;
        }

        private void OnDestroy()
        {
            if (_networkTrafficStatistics != null)
                _networkTrafficStatistics.OnNetworkTraffic -= NetworkTrafficStatistics_OnNetworkTraffic;
        }

        /// <summary>
        /// Sets a new number of seconds to average from.
        /// </summary>
        public void SetSecondsAveraged(byte seconds)
        {
            // Get to ticks.
            NetworkManager manager = InstanceFinder.NetworkManager;
            if (manager == null)
                return;

            if (seconds <= 0)
                seconds = 1;

            //Convert to milliseconds.
            long ms = seconds * 1000;
            
            uint ticks = manager.TimeManager.TimeToTicks(ms, TickRounding.RoundUp);
            
            // Should not ever be possible.
            if (ticks <= 0)
                ticks = 60;

            ClientAverages = new((int)ticks);
            ServerAverages = new((int)ticks);
        }

        /// <summary>
        /// Called when new traffic statistics are received.
        /// </summary>
        private void NetworkTrafficStatistics_OnNetworkTraffic(uint tick, BidirectionalNetworkTraffic serverTraffic, BidirectionalNetworkTraffic clientTraffic)
        {
            if (!_initialized)
                return;

            ServerAverages.AddIn(serverTraffic.InboundTraffic.Bytes);
            ServerAverages.AddOut(serverTraffic.OutboundTraffic.Bytes);

            ClientAverages.AddIn(clientTraffic.InboundTraffic.Bytes);
             
            ClientAverages.AddOut(clientTraffic.OutboundTraffic.Bytes);
            
            if (Time.time < _nextServerTextUpdateTime)
                return;

            _nextServerTextUpdateTime = Time.time + _updateInterval;

            string nl = System.Environment.NewLine;
            string result = string.Empty;

            if (_showIncoming)
                result += $"Server In: {NetworkTrafficStatistics.FormatBytesToLargest(ServerAverages.GetTotal(inBuffer: true))}/s{nl}";
            if (_showOutgoing)
                result += $"Server Out: {NetworkTrafficStatistics.FormatBytesToLargest(ServerAverages.GetTotal(inBuffer: false))}/s{nl}";

            _serverText = result;

            result = string.Empty;

            if (_showIncoming)
                result += $"Client In: {NetworkTrafficStatistics.FormatBytesToLargest(ClientAverages.GetTotal(inBuffer: true))}/s{nl}";
            if (_showOutgoing)
                result += $"Client Out: {NetworkTrafficStatistics.FormatBytesToLargest(ClientAverages.GetTotal(inBuffer: false))}/s{nl}";

            _clientText = result;
        }

        /// <summary>
        /// Called when client network traffic is updated.
        /// </summary>
        private void NetworkTraffic_OnClientNetworkTraffic(BidirectionalNetworkTraffic traffic)
        {
            if (!_initialized)
                return;

            ClientAverages.AddIn(traffic.InboundTraffic.Bytes);
            ClientAverages.AddOut(traffic.OutboundTraffic.Bytes);

            if (Time.time < _nextClientTextUpdateTime)
                return;
            _nextClientTextUpdateTime = Time.time + _updateInterval;

            string nl = System.Environment.NewLine;
            string result = string.Empty;

            if (_showIncoming)
                result += $"Client In: {NetworkTrafficStatistics.FormatBytesToLargest(ClientAverages.GetAverage(inBuffer: true))}/s{nl}";
            if (_showOutgoing)
                result += $"Client Out: {NetworkTrafficStatistics.FormatBytesToLargest(ClientAverages.GetAverage(inBuffer: false))}/s{nl}";

            _clientText = result;
        }

        /// <summary>
        /// Called when server network traffic is updated.
        /// </summary>
        private void NetworkTraffic_OnServerNetworkTraffic(BidirectionalNetworkTraffic traffic)
        {
            if (!_initialized)
                return;

            ServerAverages.AddIn(traffic.InboundTraffic.Bytes);
            ServerAverages.AddOut(traffic.OutboundTraffic.Bytes);

            if (Time.time < _nextServerTextUpdateTime)
                return;
            _nextServerTextUpdateTime = Time.time + _updateInterval;

            string nl = System.Environment.NewLine;
            string result = string.Empty;

            if (_showIncoming)
                result += $"Server In: {NetworkTrafficStatistics.FormatBytesToLargest(ServerAverages.GetAverage(inBuffer: true))}/s{nl}";
            if (_showOutgoing)
                result += $"Server Out: {NetworkTrafficStatistics.FormatBytesToLargest(ServerAverages.GetAverage(inBuffer: false))}/s{nl}";

            _serverText = result;
        }

        private void OnGUI()
        {
            _style.normal.textColor = _color;
            _style.fontSize = 15;

            float width = 100f;
            float height = 0f;
            if (_showIncoming)
                height += 15f;
            if (_showOutgoing)
                height += 15f;

            bool isClient = InstanceFinder.IsClientStarted;
            bool isServer = InstanceFinder.IsServerStarted;
            if (!isClient)
                ResetCalculationsAndDisplay(forServer: false);
            if (!isServer)
                ResetCalculationsAndDisplay(forServer: true);
            if (isServer && isClient)
                height *= 2f;

            float edge = 10f;

            float horizontal;
            float vertical;

            if (_placement == Corner.TopLeft)
            {
                horizontal = 10f;
                vertical = 10f;
                _style.alignment = TextAnchor.UpperLeft;
            }
            else if (_placement == Corner.TopRight)
            {
                horizontal = Screen.width - width - edge;
                vertical = 10f;
                _style.alignment = TextAnchor.UpperRight;
            }
            else if (_placement == Corner.BottomLeft)
            {
                horizontal = 10f;
                vertical = Screen.height - height - edge;
                _style.alignment = TextAnchor.LowerLeft;
            }
            else
            {
                horizontal = Screen.width - width - edge;
                vertical = Screen.height - height - edge;
                _style.alignment = TextAnchor.LowerRight;
            }

            GUI.Label(new(horizontal, vertical, width, height), _clientText + _serverText, _style);
        }

        [ContextMenu("Reset Averages")]
        public void ResetAverages()
        {
            ResetCalculationsAndDisplay(forServer: true);
            ResetCalculationsAndDisplay(forServer: false);
        }

        private void ResetCalculationsAndDisplay(bool forServer)
        {
            if (!_initialized)
                return;

            if (forServer)
            {
                _serverText = string.Empty;
                ServerAverages.ResetState();
            }
            else
            {
                _clientText = string.Empty;
                ClientAverages.ResetState();
            }
        }
        #endif
    }
}
#if UNITY_EDITOR || DEVELOPMENT_BUILD
#define DEVELOPMENT
#endif
using System;
using System.Collections.Generic;
using FishNet.Editing;
using FishNet.Editing.NetworkProfiler;
using FishNet.Transporting;
using GameKit.Dependencies.Utilities;
using Unity.Profiling;
using UnityEngine;

namespace FishNet.Managing.Statistic
{
    [Serializable]
    public partial class NetworkTrafficStatistics
    {
        #region Types.
        public enum EnabledMode : byte
        {
            /// <summary>
            /// Not enabled.
            /// </summary>
            Disabled = 0,
            /// <summary>
            /// Enabled for development only.
            /// </summary>
            Development = 1,
            /// <summary>
            /// Enabled for release and development.
            /// </summary>
            Release = 2,
            /// <summary>
            /// Enable for release, development, and headless.
            /// </summary>
            Headless = 3,
        }
        #endregion

        #region Public.
        /// <summary>
        /// Called when NetworkTraffic is updated.
        /// </summary>
        /// <remarks>This API is for internal use and may change at any time.</remarks>
        public event NetworkTrafficUpdateDel OnNetworkTraffic;

        public delegate void NetworkTrafficUpdateDel(uint tick, BidirectionalNetworkTraffic serverTraffic, BidirectionalNetworkTraffic clientTraffic);
        #endregion

        #region Serialized.
        /// <summary>
        /// When to enable network traffic statistics.
        /// </summary>
        public EnabledMode EnableMode => _enableMode;
        [Tooltip("When to enable network traffic statistics.")]
        [SerializeField]
        private EnabledMode _enableMode = EnabledMode.Disabled;
        /// <summary>
        /// True to update client statistics.
        /// </summary>
        public bool UpdateClient
        {
            get => _updateClient;
            private set => _updateClient = value;
        }
        [Tooltip("True to update client statistics.")]
        [SerializeField]
        private bool _updateClient;

        /// <summary>
        /// Sets UpdateClient value.
        /// </summary>
        /// <param name = "update"></param>
        public void SetUpdateClient(bool update) => UpdateClient = update;

        /// <summary>
        /// True to update client statistics.
        /// </summary>
        public bool UpdateServer
        {
            get => _updateServer;
            private set => _updateServer = value;
        }
        [Tooltip("True to update server statistics.")]
        [SerializeField]
        private bool _updateServer;

        /// <summary>
        /// Sets UpdateServer value.
        /// </summary>
        /// <param name = "update"></param>
        public void SetUpdateServer(bool update) => UpdateServer = update;
        #endregion

        #region Private.
        /// <summary>
        /// NetworkManager for this statistics.
        /// </summary>
        private NetworkManager _networkManager;
        /// <summary>
        /// Latest tick statistics for data on the local server.
        /// </summary>
        private BidirectionalNetworkTraffic _serverTraffic;
        /// <summary>
        /// Latest tick statistics for data on the local client.
        /// </summary>
        private BidirectionalNetworkTraffic _clientTraffic;
        /// <summary>
        /// Size suffixes as text.
        /// </summary>
        private static readonly string[] _sizeSuffixes = { "B", "KB", "MB", "GB", "TB", "PB", "EB", "ZB", "YB" };
        /// <summary>
        /// True if initialized.
        /// </summary>
        private bool _initializedOnce;
        #endregion

        #region Private Profiler Markers
        private static readonly ProfilerMarker _pm_OnPreTick = new("NetworkTrafficStatistics.TimeManager_OnPreTick()");
        #endregion

        #region Consts.
        /// <summary>
        /// Id for unspecified packets.
        /// </summary>
        internal const PacketId UNSPECIFIED_PACKETID = (PacketId)ushort.MaxValue;
        #endregion

        internal void InitializeOnce_Internal(NetworkManager manager)
        {
            if (_initializedOnce)
                return;

            /* Only subscribe if enabled. Always unsubscribe even if not enabled -- doing so is safe. */
            if (!IsEnabled())
                return;

            manager.TimeManager.OnPreTick += TimeManager_OnPreTick;

            _networkManager = manager;

            /* Do not bother caching once destroyed. Losing a single instance of each
             * isn't going to hurt anything, and if destroyed everything is probably
             * shutting down anyway. */
            _serverTraffic = ResettableObjectCaches<BidirectionalNetworkTraffic>.Retrieve();
            _clientTraffic = ResettableObjectCaches<BidirectionalNetworkTraffic>.Retrieve();

            _initializedOnce = true;
        }

        /// <summary>
        /// Called before the TimeManager ticks.
        /// </summary>
        private void TimeManager_OnPreTick()
        {
            using (_pm_OnPreTick.Auto())
            {
                /* Since we are sending last ticks data at the end of the tick,
                 * the tick used will always be 1 less than current tick. */
                long trafficTick = _networkManager.TimeManager.LocalTick - 1;
                //Invalid tick.
                if (trafficTick <= 0)
                    return;

                if (_networkManager.IsClientStarted || _networkManager.IsServerStarted)
                    OnNetworkTraffic?.Invoke((uint)trafficTick, _serverTraffic, _clientTraffic);

                /* It's important to remember that after actions are invoked
                 * the traffic stat fields are reset. Each listener should use
                 * the MultiwayTrafficCollection.Clone method to get a copy,
                 * and should cache that copy when done. */
                _clientTraffic.Reinitialize();
                _serverTraffic.Reinitialize();
            }
        }

        /// <summary>
        /// Called when a packet bundle is received. This is any number of packets bundled into a single transmission.
        /// </summary>
        internal void PacketBundleReceived(bool asServer)
        {
            //Debug.LogError("Inbound and outbound bidirection datas should count up how many packet bundles are received. This is so the bundle headers can be calculated appropriately.");
        }

        /// <summary>
        /// Called when data is being sent from the local server or client for a specific packet.
        /// </summary>
        internal void AddOutboundPacketIdData(PacketId typeSource, string details, int bytes, GameObject gameObject, bool asServer)
        {
            if (bytes <= 0)
                return;

            if (TryGetBidirectionalNetworkTraffic(asServer, out BidirectionalNetworkTraffic networkTraffic))
                networkTraffic.OutboundTraffic.AddPacketIdData(typeSource, details, (ulong)bytes, gameObject);
        }

        /// <summary>
        /// Called when data is being sent from the local server or client as it's going to the socket.
        /// </summary>
        internal void AddOutboundSocketData(ulong bytes, bool asServer)
        {
            if (bytes > int.MaxValue)
                bytes = int.MaxValue;
            else if (bytes <= 0)
                return;

            if (TryGetBidirectionalNetworkTraffic(asServer, out BidirectionalNetworkTraffic networkTraffic))
                networkTraffic.OutboundTraffic.AddSocketData(bytes);
        }

        /// <summary>
        /// Called when data is being received on the local server or client for a specific packet.
        /// </summary>
        internal void AddInboundPacketIdData(PacketId typeSource, string details, int bytes, GameObject gameObject, bool asServer)
        {
            if (bytes <= 0)
                return;

            if (TryGetBidirectionalNetworkTraffic(asServer, out BidirectionalNetworkTraffic networkTraffic))
                networkTraffic.InboundTraffic.AddPacketIdData(typeSource, details, (ulong)bytes, gameObject);
        }

        /// <summary>
        /// Called when data is being received on the local server or client as it's coming from the socket.
        /// </summary>
        internal void AddInboundSocketData(ulong bytes, bool asServer)
        {
            if (bytes > int.MaxValue)
                bytes = int.MaxValue;
            else if (bytes <= 0)
                return;

            if (TryGetBidirectionalNetworkTraffic(asServer, out BidirectionalNetworkTraffic networkTraffic))
                networkTraffic.InboundTraffic.AddSocketData(bytes);
        }

        /// <summary>
        /// Gets current statistics for server or client.
        /// </summary>
        private bool TryGetBidirectionalNetworkTraffic(bool asServer, out BidirectionalNetworkTraffic networkTraffic)
        {
            networkTraffic = asServer ? _serverTraffic : _clientTraffic;

            return networkTraffic != null;
        }

        /// <summary>
        /// Formats passed in bytes value to the largest possible data type with 2 decimals.
        /// </summary>
        public static string FormatBytesToLargest(float bytes)
        {
            string[] units = { "B", "kB", "MB", "GB", "TB", "PB" };
            int unitIndex = 0;

            while (bytes >= 1024 && unitIndex < units.Length - 1)
            {
                bytes /= 1024;
                unitIndex++;
            }

            return $"{bytes:0.00} {units[unitIndex]}";
        }

        /// <summary>
        /// Returns if enabled or not.
        /// </summary>
        public bool IsEnabled()
        {
            if (_enableMode == EnabledMode.Disabled)
                return false;

            int modeValue = (int)_enableMode;

            //Never enabled for server builds.
            #if UNITY_SERVER
            return modeValue >= (int)EnabledMode.Headless;
            #endif

            if (_enableMode == EnabledMode.Disabled)
                return false;

            // If not in dev mode then return true if to run in release.
            #if !DEVELOPMENT
            return modeValue >= (int)EnabledMode.Release;
            // Always run in dev mode if not disabled.
            #else
            return true;
            #endif
        }
    }
}
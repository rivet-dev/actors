using FishNet.Managing.Timing;
using GameKit.Dependencies.Utilities;

namespace FishNet.Editing.NetworkProfiler
{
    /// <summary>
    /// Data for a profiled tick. 
    /// </summary>
    internal class ProfiledTickData : IResettable
    {
        /// <summary>
        /// Tick this is for.
        /// </summary>
        public uint Tick;
        /// <summary>
        /// Traffic collection for the server.
        /// </summary>
        public BidirectionalNetworkTraffic ServerTraffic;
        /// <summary>
        /// Traffic collection for the client.
        /// </summary>
        public BidirectionalNetworkTraffic ClientTraffic;

        /// <summary>
        /// Initializes and returns if successful.
        /// </summary>
        public bool TryInitialize(uint tick, BidirectionalNetworkTraffic serverTraffic, BidirectionalNetworkTraffic clientTraffic)
        {
            Tick = tick;

            ServerTraffic = serverTraffic.CloneUsingCache();
            ClientTraffic = clientTraffic.CloneUsingCache();

            return ServerTraffic != null && ClientTraffic != null;
        }

        /// <summary>
        /// Resets all values and stores to caches as needed.
        /// </summary>
        public void ResetState()
        {
            Tick = TimeManager.UNSET_TICK;

            ResettableObjectCaches<BidirectionalNetworkTraffic>.StoreAndDefault(ref ServerTraffic);
            ResettableObjectCaches<BidirectionalNetworkTraffic>.StoreAndDefault(ref ClientTraffic);
        }

        public void InitializeState() { }
    }
}
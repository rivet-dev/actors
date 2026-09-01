using System.Collections.Generic;
using FishNet.Managing;
using FishNet.Managing.Statistic;
using FishNet.Transporting;
using GameKit.Dependencies.Utilities;
using UnityEngine;

namespace FishNet.Editing.NetworkProfiler
{
    /// <summary>
    /// Used to store Inbound and Outbound traffic details.
    /// </summary>
    public class BidirectionalNetworkTraffic : IResettable
    {
        /// <summary>
        /// Received traffic.
        /// </summary>
        internal NetworkTraffic InboundTraffic;
        /// <summary>
        /// Sent traffic.
        /// </summary>
        internal NetworkTraffic OutboundTraffic;

        /// <summary>
        /// Creates a clone of this class using cache.
        /// </summary>
        /// <returns></returns>
        public BidirectionalNetworkTraffic CloneUsingCache()
        {
            if (InboundTraffic == null)
            {
                NetworkManagerExtensions.LogError($"One or more NetworkTraffic values is null. {nameof(BidirectionalNetworkTraffic)} cannot be cloned.");
                return null;
            }

            BidirectionalNetworkTraffic traffic = ResettableObjectCaches<BidirectionalNetworkTraffic>.Retrieve();

            traffic.InboundTraffic = InboundTraffic;
            traffic.OutboundTraffic = OutboundTraffic;

            return traffic;
        }

        /// <summary>
        /// Re-initializes by calling ResetState, then InitializeState.
        /// </summary>
        public void Reinitialize()
        {
            ResetState();
            InitializeState();
        }

        public void ResetState()
        {
            ResettableObjectCaches<NetworkTraffic>.StoreAndDefault(ref InboundTraffic);
            ResettableObjectCaches<NetworkTraffic>.StoreAndDefault(ref OutboundTraffic);
        }

        public void InitializeState()
        {
            InboundTraffic = ResettableObjectCaches<NetworkTraffic>.Retrieve();
            OutboundTraffic = ResettableObjectCaches<NetworkTraffic>.Retrieve();
        }
    }
    
}
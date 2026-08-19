using FishNet.Object;
using FishNet.Transporting;
using System;
using System.Collections.Generic;
using UnityEngine;

namespace FishNet.Managing.Server
{
    public sealed partial class ServerManager : MonoBehaviour
    {
        #region Internal
        /// <summary>
        /// Current RPCLinks.
        /// </summary>
        internal Dictionary<ushort, RpcLink> RpcLinks = new();
        /// <summary>
        /// RPCLink indexes which can be used.
        /// </summary>
        private Queue<ushort> _availableRpcLinkIndexes = new();
        #endregion

        /// <summary>
        /// Initializes RPC Links for NetworkBehaviours.
        /// </summary>
        private void InitializeRpcLinks()
        {
            ushort startingLink = NetworkManager.StartingRpcLinkIndex;
            for (ushort i = ushort.MaxValue; i >= startingLink; i--)
                _availableRpcLinkIndexes.Enqueue(i);
        }

        /// <summary>
        /// Sets the next RPC Link to use.
        /// </summary>
        /// <returns>True if a link was available and set.</returns>
        internal bool GetRpcLink(out ushort value)
        {
            if (_availableRpcLinkIndexes.Count > 0)
            {
                value = _availableRpcLinkIndexes.Dequeue();
                return true;
            }
            else
            {
                value = 0;
                return false;
            }
        }

        /// <summary>
        /// Sets data to RpcLinks for linkIndex.
        /// </summary>
        internal void SetRpcLink(ushort linkIndex, RpcLink data)
        {
            RpcLinks[linkIndex] = data;
        }

        /// <summary>
        /// Returns RPCLinks to availableRpcLinkIndexes.
        /// </summary>
        internal void StoreRpcLinks(Dictionary<uint, RpcLinkType> links)
        {
            foreach (RpcLinkType rlt in links.Values)
                _availableRpcLinkIndexes.Enqueue(rlt.LinkPacketId);
        }
    }
}
#if UNITY_EDITOR || DEVELOPMENT_BUILD
#define DEVELOPMENT
#endif
using FishNet.Managing.Server;
using FishNet.Object.Helping;
using FishNet.Serializing;
using FishNet.Transporting;
using System.Collections.Generic;
using System.Linq;
using System.Runtime.CompilerServices;
using UnityEngine;

namespace FishNet.Object
{
    public abstract partial class NetworkBehaviour : MonoBehaviour
    {
        #region Private.
        /// <summary>
        /// Link indexes for RPCs.
        /// </summary>
        private Dictionary<uint, RpcLinkType> _rpcLinks = new();
        #endregion

        #region Consts.
        /// <summary>
        /// Number of bytes written for each RPCLinks.
        /// </summary>
        internal const int RPCLINK_RESERVED_BYTES = 2;
        #endregion

        /// <summary>
        /// Initializes RpcLinks. This will only call once even as host.
        /// </summary>
        private void InitializeRpcLinks()
        {
            /* Link only data from server to clients. While it is
             * just as easy to link client to server it's usually
             * not needed because server out data is more valuable
             * than server in data. */
            /* Links will be stored in the NetworkBehaviour so that
             * when the object is destroyed they can be added back
             * into availableRpcLinks, within the ServerManager. */

            ServerManager serverManager = NetworkManager.ServerManager;
            // ObserverRpcs.
            if (_observersRpcDelegates != null)
            {
                foreach (uint rpcHash in _observersRpcDelegates.Keys)
                {
                    if (!MakeLink(rpcHash, PacketId.ObserversRpc))
                        return;
                }
            }
            // TargetRpcs.
            if (_targetRpcDelegates != null)
            {
                foreach (uint rpcHash in _targetRpcDelegates.Keys)
                {
                    if (!MakeLink(rpcHash, PacketId.TargetRpc))
                        return;
                }
            }
            //ReconcileRpcs.
            if (_reconcileRpcDelegates != null)
            {
                foreach (uint rpcHash in _reconcileRpcDelegates.Keys)
                {
                    if (!MakeLink(rpcHash, PacketId.Reconcile))
                        return;
                }
            }

            /* Tries to make a link and returns if
             * successful. When a link cannot be made the method
             * should exit as no other links will be possible. */
            bool MakeLink(uint rpcHash, PacketId packetId)
            {
                if (serverManager.GetRpcLink(out ushort linkIndex))
                {
                    _rpcLinks[rpcHash] = new(rpcHash, packetId, linkIndex);
                    return true;
                }
                else
                {
                    return false;
                }
            }
        }

        /// <summary>
        /// Returns an estimated length for any Rpc header.
        /// </summary>
        /// <returns></returns>
        private int GetEstimatedRpcHeaderLength()
        {
            /* Imaginary number for how long RPC headers are.
             * They are well under this value but this exist to
             * ensure a writer of appropriate length is pulled
             * from the pool. */
            return 20;
        }

        /// <summary>
        /// Creates a PooledWriter and writes the header for a rpc.
        /// </summary>
        private PooledWriter CreateLinkedRpc(RpcLinkType link, PooledWriter methodWriter, Channel channel)
        {
            int rpcHeaderBufferLength = GetEstimatedRpcHeaderLength();
            int methodWriterLength = methodWriter.Length;
            //Writer containing full packet.
            PooledWriter writer = WriterPool.Retrieve(rpcHeaderBufferLength + methodWriterLength);
            writer.WriteUInt16(link.LinkPacketId);

#if DEVELOPMENT
            int written = WriteDebugForValidateRpc(writer, link.RpcPacketId, link.RpcHash);
#endif

            //Write length only if reliable.
            if (channel == Channel.Reliable)
                writer.WriteInt32(methodWriter.Length);
            //Data.
            writer.WriteArraySegment(methodWriter.GetArraySegment());

#if DEVELOPMENT
            WriteDebugLengthForValidateRpc(writer, written);
#endif

            return writer;
        }

        /// <summary>
        /// Returns RpcLinks the ServerManager.
        /// </summary>
        private void ReturnRpcLinks()
        {
            if (_rpcLinks.Count == 0)
                return;

            ServerManager?.StoreRpcLinks(_rpcLinks);
            _rpcLinks.Clear();
        }

        /// <summary>
        /// Writes rpcLinks to writer.
        /// </summary>
        internal void WriteRpcLinks(Writer writer)
        {
            int rpcLinksCount = _rpcLinks.Count;
            if (rpcLinksCount == 0)
                return;

            writer.WriteNetworkBehaviourId(this);
            writer.WriteUInt16((ushort)rpcLinksCount);

            foreach (KeyValuePair<uint, RpcLinkType> item in _rpcLinks)
            {
                //RpcLink index.
                writer.WriteUInt16Unpacked(item.Value.LinkPacketId);
                //Hash.
                writer.WriteUInt16Unpacked((ushort)item.Key);
                //True/false if observersRpc.
                writer.WriteUInt16Unpacked((ushort)item.Value.RpcPacketId);
            }
        }
    }
}
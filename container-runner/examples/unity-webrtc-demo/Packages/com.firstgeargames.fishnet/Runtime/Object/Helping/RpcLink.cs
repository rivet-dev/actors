using FishNet.Object.Helping;
using FishNet.Transporting;

namespace FishNet.Object
{
    #region Types.
    /// <summary>
    /// Lookup data for a RPC Link.
    /// </summary>
    internal readonly struct RpcLink
    {
        /// <summary>
        /// ObjectId for link.
        /// </summary>
        public readonly int ObjectId;
        /// <summary>
        /// NetworkBehaviour component index on ObjectId.
        /// </summary>
        public readonly byte ComponentIndex;
        /// <summary>
        /// RpcHash for link.
        /// </summary>
        public readonly uint RpcHash;
        /// <summary>
        /// PacketId used for the Rpc type when not using links.
        /// </summary>
        public readonly PacketId RpcPacketId;

        public RpcLink(int objectId, byte componentIndex, uint rpcHash, PacketId packetId)
        {
            ObjectId = objectId;
            ComponentIndex = componentIndex;
            RpcHash = rpcHash;
            RpcPacketId = packetId;
        }
    }
    #endregion
}
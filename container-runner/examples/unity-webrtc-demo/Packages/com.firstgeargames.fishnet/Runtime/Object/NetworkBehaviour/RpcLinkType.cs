using FishNet.Object.Helping;
using FishNet.Transporting;

namespace FishNet.Object
{
    internal struct RpcLinkType
    {
        /// <summary>
        /// Hash for the Rpc.
        /// </summary>
        public readonly uint RpcHash;
        /// <summary>
        /// PacketId used for the Rpc type when not using links.
        /// </summary>
        public readonly PacketId RpcPacketId;
        /// <summary>
        /// PacketId sent for the RpcLink.
        /// </summary>
        public readonly ushort LinkPacketId;

        public RpcLinkType(uint rpcHash, PacketId packetId, ushort linkPacketId)
        {
            RpcHash = rpcHash;
            RpcPacketId = packetId;
            LinkPacketId = linkPacketId;
        }
    }
}
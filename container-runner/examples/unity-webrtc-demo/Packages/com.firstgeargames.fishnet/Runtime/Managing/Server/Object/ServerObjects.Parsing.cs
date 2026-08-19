#if UNITY_EDITOR || DEVELOPMENT_BUILD
#define DEVELOPMENT
#endif
using FishNet.Connection;
using FishNet.Managing.Object;
using FishNet.Managing.Utility;
using FishNet.Object;
using FishNet.Serializing;
using FishNet.Transporting;
using System.Runtime.CompilerServices;

namespace FishNet.Managing.Server
{
    public partial class ServerObjects : ManagedObjects
    {
        /// <summary>
        /// Parses a ServerRpc.
        /// </summary>
        internal void ParseServerRpc(PooledReader reader, NetworkConnection conn, Channel channel)
        {
#if DEVELOPMENT
            NetworkBehaviour.ReadDebugForValidatedRpc(NetworkManager, reader, out int startReaderRemaining, out string rpcInformation, out uint expectedReadAmount);
#endif
            int readerStartAfterDebug = reader.Position;

            NetworkBehaviour nb = reader.ReadNetworkBehaviour();
            int dataLength = Packets.GetPacketLength((ushort)PacketId.ServerRpc, reader, channel);

            if (nb != null)
                nb.ReadServerRpc(readerStartAfterDebug, fromRpcLink: false, hash: 0, reader, conn, channel);
            else
                SkipDataLength((ushort)PacketId.ServerRpc, reader, dataLength);

#if DEVELOPMENT
            NetworkBehaviour.TryPrintDebugForValidatedRpc(fromRpcLink: false, NetworkManager, reader, startReaderRemaining, rpcInformation, expectedReadAmount, channel);
#endif
        }
    }
}
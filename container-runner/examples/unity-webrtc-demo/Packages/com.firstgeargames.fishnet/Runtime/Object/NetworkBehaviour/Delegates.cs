using FishNet.Connection;
using FishNet.Serializing;
using FishNet.Transporting;
using FishNet.Utility;
using System.Runtime.CompilerServices;

[assembly: InternalsVisibleTo(UtilityConstants.CODEGEN_ASSEMBLY_NAME)]

namespace FishNet.Object.Delegating
{
    public delegate void ServerRpcDelegate(PooledReader reader, Channel channel, NetworkConnection sender);

    public delegate void ClientRpcDelegate(PooledReader reader, Channel channel);

    public delegate bool SyncVarReadDelegate(PooledReader reader, byte index, bool asServer);
}
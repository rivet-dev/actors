using FishNet.Connection;
using FishNet.Documenting;
using FishNet.Serializing;
using FishNet.Transporting;
using FishNet.Utility;
using System.Runtime.CompilerServices;

[assembly: InternalsVisibleTo(UtilityConstants.CODEGEN_ASSEMBLY_NAME)]

namespace FishNet.Object.Prediction.Delegating
{
    [APIExclude]
    public delegate void ReplicateRpcDelegate(PooledReader reader, NetworkConnection sender, Channel channel);

    [APIExclude]
    public delegate void ReconcileRpcDelegate(PooledReader reader, Channel channel);

    [APIExclude]
    public delegate void ReplicateUserLogicDelegate<T>(T data, ReplicateState state, Channel channel);

    [APIExclude]
    public delegate void ReconcileUserLogicDelegate<T>(T data, Channel channel);
}
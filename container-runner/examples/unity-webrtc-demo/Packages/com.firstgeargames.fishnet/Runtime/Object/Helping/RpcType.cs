namespace FishNet.Object.Helping
{
    public enum RpcType : int
    {
        None = 0,
        Server = 1,
        Observers = 2,
        Target = 4,
        Replicate = 8,
        Reconcile = 16
    }
}
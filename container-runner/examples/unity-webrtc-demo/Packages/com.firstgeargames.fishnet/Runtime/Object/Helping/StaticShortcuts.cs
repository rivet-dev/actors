namespace FishNet.Object.Helping
{
    public static class CodegenHelper
    {
        /// <summary>
        /// Returns if a NetworkObject is deinitializing.
        /// </summary>
        /// <param name = "nb"></param>
        /// <returns></returns>
        public static bool NetworkObject_Deinitializing(NetworkBehaviour nb)
        {
            if (nb == null)
                return true;

            return nb.IsDeinitializing;
        }

        /// <summary>
        /// Returns if running as server.
        /// </summary>
        /// <param name = "nb"></param>
        /// <returns></returns>
        public static bool IsServer(NetworkBehaviour nb)
        {
            if (nb == null)
                return false;

            return nb.IsServerStarted;
        }

        /// <summary>
        /// Returns if running as client.
        /// </summary>
        /// <param name = "nb"></param>
        /// <returns></returns>
        public static bool IsClient(NetworkBehaviour nb)
        {
            if (nb == null)
                return false;

            return nb.IsClientStarted;
        }
    }
}
using System.Net;

namespace SynapseSocket.Core
{

    public static class IPEndPointExtensions
    {
        public static int GetAddressPortHashcode(this IPEndPoint endPoint)
        {
            if (endPoint is null)
                return 0;

            int hashCode = unchecked(endPoint.Address.GetHashCode() * 397) ^ endPoint.Port;
            return hashCode;
        }
    }
}

using FishNet.Object;
using System.Collections.Generic;
using UnityEngine;

namespace FishNet.Utility.Performance
{
    public static class GetNonAlloc
    {
        /// <summary>
        /// Gets all NetworkBehaviours on a transform.
        /// </summary>
        public static void GetNetworkBehavioursNonAlloc(this Transform t, ref List<NetworkBehaviour> results)
        {
            t.GetComponents(results);
        }

        /// <summary>
        /// Gets all transforms on transform and it's children.
        /// </summary>
        public static void GetTransformsInChildrenNonAlloc(this Transform t, ref List<Transform> results, bool includeInactive = false)
        {
            t.GetComponentsInChildren(includeInactive, results);
        }
    }
}
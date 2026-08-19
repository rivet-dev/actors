using System.Collections.Generic;
using FishNet.Managing.Scened;
using UnityEngine.SceneManagement;

namespace FishNet.Serializing.Helping
{
    internal sealed class SceneHandleEqualityComparer : EqualityComparer<Scene>
    {
        public override bool Equals(Scene a, Scene b)
        {
            return SceneHandleCompatibility.Equals(a, b);
        }

        public override int GetHashCode(Scene obj)
        {
            return SceneHandleCompatibility.ToInt(obj);
        }
    }
}

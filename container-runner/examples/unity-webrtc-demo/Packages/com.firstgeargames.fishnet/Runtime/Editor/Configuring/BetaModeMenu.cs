#if UNITY_EDITOR
using UnityEditor;
using UnityEngine;

namespace FishNet.Editing.Beta
{
    public class BetaModeMenu : MonoBehaviour
    {
        #region const.
        private const string STABLE_RECURSIVE_DESPAWNS_DEFINE = "FISHNET_STABLE_RECURSIVE_DESPAWNS";
        private const string THREADED_TICKSMOOTHERS_DEFINE = "FISHNET_THREADED_TICKSMOOTHERS";
        private const string THREADED_COLLIDER_ROLLBACK_DEFINE = "FISHNET_THREADED_COLLIDER_ROLLBACK";
        #endregion

        #region Beta Recursive Despawns
        #if FISHNET_STABLE_RECURSIVE_DESPAWNS
        [MenuItem("Tools/Fish-Networking/Beta/Enable Recursive Despawns", false, -1101)]
        private static void EnableBetaRecursiveDespawns() => SetBetaRecursiveDespawns(useStable: false);
        #else
        [MenuItem("Tools/Fish-Networking/Beta/Disable Recursive Despawns", false, -1101)]
        private static void DisableBetaRecursiveDespawns() => SetBetaRecursiveDespawns(useStable: true);
        #endif
        private static void SetBetaRecursiveDespawns(bool useStable)
        {
            bool result = DeveloperMenu.RemoveOrAddDefine(STABLE_RECURSIVE_DESPAWNS_DEFINE, removeDefine: !useStable);
            if (result)
                Debug.LogWarning($"Beta Recursive Despawns are now {GetBetaEnabledText(useStable)}.");
        }
        #endregion

        #region Beta ThreadedSmothers
        /* Changes by https://github.com/belplaton
         * Content: Threaded TickSmoothers
         *      Migrating the network interpolation system for the graphical world to a multithreaded Unity Jobs + Burst implementation. */
        #if FISHNET_THREADED_TICKSMOOTHERS
        [MenuItem("Tools/Fish-Networking/Beta/Disable Threaded TickSmoothers", false, -1101)]
        private static void DisableBetaThreadedSmoothers() => SetBetaThreadedSmoothers(useStable: true);
        #else
        [MenuItem("Tools/Fish-Networking/Beta/Enable Threaded TickSmoothers", false, -1101)]
        private static void EnableBetaThreadedSmoothers()
        {
            #if UNITYMATHEMATICS || UNITYMATHEMATICS_131 || UNITYMATHEMATICS_132
            SetBetaThreadedSmoothers(useStable: false);
            #else
            Debug.LogError($"You must install the package com.unity.mathematics to use Beta Threaded TickSmoothers.");
            #endif
        }
        #endif

        private static void SetBetaThreadedSmoothers(bool useStable)
        {
            bool result = DeveloperMenu.RemoveOrAddDefine(THREADED_TICKSMOOTHERS_DEFINE, removeDefine: useStable);
            if (result)
                Debug.LogWarning($"Beta Threaded TickSmoothers are now {GetBetaEnabledText(useStable)}.");
        }
        #endregion

        #region Beta Threaded Collider Rollback
        /* Changes by https://github.com/belplaton
         * Content: Threaded Collider Rollback
         *      Migrating collider rollback -- commonly used for hitbox tracing -- to a multithreaded Unity Jobs + Burst implementation. */
        #if FISHNET_THREADED_COLLIDER_ROLLBACK
        [MenuItem("Tools/Fish-Networking/Beta/Disable Threaded Collider Rollback", false, -1101)]
        private static void DisableBetaThreadedColliderRollback() => SetBetaThreadedColliderRollback(useStable: true);
        #else
        [MenuItem("Tools/Fish-Networking/Beta/Enable Threaded Collider Rollback", false, -1101)]
        private static void EnableBetaThreadedColliderRollback()
        {
            #if UNITYMATHEMATICS || UNITYMATHEMATICS_131 || UNITYMATHEMATICS_132
            SetBetaThreadedColliderRollback(useStable: false);
            #else
            Debug.LogError($"You must install the package com.unity.mathematics to use Beta Threaded Collider Rollhack..");
            #endif
        }
        #endif

        private static void SetBetaThreadedColliderRollback(bool useStable)
        {
            bool result = DeveloperMenu.RemoveOrAddDefine(THREADED_COLLIDER_ROLLBACK_DEFINE, removeDefine: useStable);
            if (result)
                Debug.LogWarning($"Beta Threaded Collider Rollbacks are now {GetBetaEnabledText(useStable)}.");
        }
        #endregion

        private static string GetBetaEnabledText(bool useStable)
        {
            return useStable ? "disabled" : "enabled";
        }
    }
}

#endif
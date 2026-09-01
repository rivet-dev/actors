#if UNITY_EDITOR
using UnityEditor;
#endif
using UnityEngine;

namespace GameKit.Dependencies.Utilities.ObjectPooling.Examples
{
    public class Projectile : MonoBehaviour
    {
        /// <summary>
        /// If above 0f projectiles are stored with a delay rather than when off screen.
        /// </summary>
        [Tooltip("If above 0f projectiles are stored with a delay rather than when off screen.")]
        [Range(0f, 5f)]
        public float DestroyDelay = 0f;
        public float MoveRate = 30f;
        private ProjectileSpawner _spawner;
        private MeshRenderer[] _renderers;
        private Vector3 _moveDirection;
        /// <summary>
        /// True if existing play mode.
        /// </summary>
        private bool _exitingPlayMode = false;

        private void Awake()
        {
#if UNITY_EDITOR
            EditorApplication.playModeStateChanged += EditorApplication_playModeStateChanged;
#endif


            // Used as our pretend overhead.
            for (int i = 0; i < 30; i++)
            {
                _spawner = FindObjectOfType<ProjectileSpawner>();
                _renderers = GetComponentsInChildren<MeshRenderer>();
            }
        }

#if UNITY_EDITOR
        /// <summary>
        /// Received when editor play mode changes.
        /// </summary>
        /// <param name = "obj"></param>
        private void EditorApplication_playModeStateChanged(PlayModeStateChange obj)
        {
            if (!EditorApplication.isPlayingOrWillChangePlaymode && EditorApplication.isPlaying)
                _exitingPlayMode = true;
        }
#endif

        private void OnBecameInvisible()
        {
            // Don't try to pool if exiting play mode. Doesn't harm anything but creates annoying errors.
            if (_exitingPlayMode)
                return;

            if (DestroyDelay <= 0f)
            {
                if (_spawner.UsePool)
                {
                    ObjectPool.Store(gameObject);
                }
                else
                {
                    Destroy(gameObject);
                }
            }
        }

        private void OnEnable()
        {
            _moveDirection = new Vector3(Random.Range(-1f, 1f), 1f, 0f).normalized;
            if (_spawner.UsePool && DestroyDelay > 0f)
                ObjectPool.Store(gameObject, DestroyDelay);
        }

        private void Update()
        {
            transform.position += _moveDirection * MoveRate * Time.deltaTime;
        }
    }
}
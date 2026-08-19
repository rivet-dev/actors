using System.IO;
using FishNet.Component.Spawning;
using FishNet.Component.Transforming;
using FishNet.Managing;
using FishNet.Managing.Client;
using FishNet.Managing.Object;
using FishNet.Managing.Server;
using FishNet.Managing.Timing;
using FishNet.Managing.Transporting;
using FishNet.Object;
using FishNet.Transporting.FishyWebRTC;
using GameKit.Dependencies.Utilities;
using System.Text;
using UnityEditor;
using UnityEditor.SceneManagement;
using UnityEngine;

public static class ProjectBootstrap
{
    public const string MainScene = "Assets/Scenes/Main.unity";
    private const string DefaultPrefabObjectsPath = "Assets/DefaultPrefabObjects.asset";
    private const string PlayerPrefabPath = "Assets/DemoAssets/NetworkPlayer.prefab";
    private const string PlayerMaterialPath = "Assets/DemoAssets/NetworkPlayer.mat";
    private const string FloorMaterialPath = "Assets/DemoAssets/Floor.mat";
    private static readonly StringBuilder HashBuilder = new();

    [MenuItem("Tools/Setup FishNet Multiplayer Demo")]
    public static void Setup()
    {
        Directory.CreateDirectory(Path.GetDirectoryName(MainScene));

        var scene = File.Exists(MainScene)
            ? EditorSceneManager.OpenScene(MainScene, OpenSceneMode.Single)
            : EditorSceneManager.NewScene(NewSceneSetup.EmptyScene, NewSceneMode.Single);

        var nmGo = GameObject.Find("NetworkManager") ?? new GameObject("NetworkManager");

        var networkManager = GetOrAdd<NetworkManager>(nmGo);
        GetOrAdd<TimeManager>(nmGo);
        var transportManager = GetOrAdd<TransportManager>(nmGo);
        GetOrAdd<ClientManager>(nmGo);
        var serverManager = GetOrAdd<ServerManager>(nmGo);
        GameObjectUtility.RemoveMonoBehavioursWithMissingScript(nmGo);
        var webRtc = GetOrAdd<FishyWebRTC>(nmGo);
        GetOrAdd<ServerBootstrap>(nmGo);
        var playerSpawner = GetOrAdd<PlayerSpawner>(nmGo);
        var playerPrefab = EnsurePlayerPrefab();
        var defaultPrefabObjects = EnsureDefaultPrefabObjects(playerPrefab);

        SetObjectField(transportManager, "Transport", webRtc);
        SetObjectField(networkManager, "_spawnablePrefabs", defaultPrefabObjects);
        SetObjectField(playerSpawner, "_playerPrefab", playerPrefab);
        SetPrivateBoolField(webRtc, "_HTTPS", false);
        SetPrivateBoolField(webRtc, "_noClientPort", false);
        SetPrivateIntField(webRtc, "_port", 7770);
        SetPrivateStringField(webRtc, "_clientAddress", "127.0.0.1");
        SetPrivateStringField(webRtc, "_clientPath", "/");

        // Startup is controlled by ServerBootstrap so the same build can run as either
        // a server or a client from command-line flags.
        SetPrivateBoolField(serverManager, "_startOnHeadless", false);
        playerSpawner.Spawns = EnsureSpawnPoints();
        EnsureCameraAndLight();
        EnsureFloor();

        EditorUtility.SetDirty(networkManager);
        EditorUtility.SetDirty(transportManager);
        EditorUtility.SetDirty(serverManager);
        EditorUtility.SetDirty(webRtc);
        EditorUtility.SetDirty(playerSpawner);
        EditorUtility.SetDirty(defaultPrefabObjects);
        EditorSceneManager.SaveScene(scene, MainScene);
        AssetDatabase.ImportAsset(MainScene);
        AddSceneToBuildSettings(MainScene);
        AssetDatabase.SaveAssets();
        AssetDatabase.Refresh();

        Debug.Log("[bootstrap] Rivet FishNet starter scene wired to FishyWebRTC with player spawning");
    }

    private static T GetOrAdd<T>(GameObject go) where T : Component
    {
        var c = go.GetComponent<T>();
        return c != null ? c : go.AddComponent<T>();
    }

    private static void SetObjectField(Object target, string field, Object value)
    {
        var so = new SerializedObject(target);
        var prop = so.FindProperty(field);
        if (prop == null)
        {
            Debug.LogWarning($"[bootstrap] field {field} not found on {target.GetType().Name}");
            return;
        }
        prop.objectReferenceValue = value;
        so.ApplyModifiedPropertiesWithoutUndo();
    }

    private static void SetPrivateBoolField(Object target, string field, bool value)
    {
        var so = new SerializedObject(target);
        var prop = so.FindProperty(field);
        if (prop == null)
        {
            Debug.LogWarning($"[bootstrap] field {field} not found on {target.GetType().Name}");
            return;
        }
        prop.boolValue = value;
        so.ApplyModifiedPropertiesWithoutUndo();
    }

    private static void SetPrivateIntField(Object target, string field, int value)
    {
        var so = new SerializedObject(target);
        var prop = so.FindProperty(field);
        if (prop == null)
        {
            Debug.LogWarning($"[bootstrap] field {field} not found on {target.GetType().Name}");
            return;
        }
        prop.intValue = value;
        so.ApplyModifiedPropertiesWithoutUndo();
    }

    private static void SetPrivateStringField(Object target, string field, string value)
    {
        var so = new SerializedObject(target);
        var prop = so.FindProperty(field);
        if (prop == null)
        {
            Debug.LogWarning($"[bootstrap] field {field} not found on {target.GetType().Name}");
            return;
        }
        prop.stringValue = value;
        so.ApplyModifiedPropertiesWithoutUndo();
    }

    private static void EnsureCameraAndLight()
    {
        if (Camera.main == null)
        {
            var cameraGo = new GameObject("Main Camera");
            cameraGo.tag = "MainCamera";
            cameraGo.transform.position = new Vector3(0f, 6f, -8f);
            cameraGo.transform.rotation = Quaternion.Euler(35f, 0f, 0f);
            var camera = cameraGo.AddComponent<Camera>();
            camera.clearFlags = CameraClearFlags.SolidColor;
            camera.backgroundColor = new Color(0.06f, 0.07f, 0.08f);
            cameraGo.AddComponent<AudioListener>();
            EditorUtility.SetDirty(cameraGo);
        }
        if (Object.FindAnyObjectByType<Light>() == null)
        {
            var lightGo = new GameObject("Directional Light");
            lightGo.transform.rotation = Quaternion.Euler(50f, -30f, 0f);
            var light = lightGo.AddComponent<Light>();
            light.type = LightType.Directional;
            light.intensity = 1.5f;
            EditorUtility.SetDirty(lightGo);
        }
    }

    private static NetworkObject EnsurePlayerPrefab()
    {
        Directory.CreateDirectory(Path.GetDirectoryName(PlayerPrefabPath));

        var existing = AssetDatabase.LoadAssetAtPath<NetworkObject>(PlayerPrefabPath);
        if (existing != null)
            return existing;

        var playerGo = GameObject.CreatePrimitive(PrimitiveType.Capsule);
        playerGo.name = "NetworkPlayer";
        playerGo.transform.position = new Vector3(0f, 1f, 0f);

        var renderer = playerGo.GetComponent<Renderer>();
        renderer.sharedMaterial = EnsurePlayerMaterial();

        playerGo.AddComponent<NetworkObject>();
        playerGo.AddComponent<NetworkTransform>();
        playerGo.AddComponent<NetworkPlayerController>();

        GameObject prefab = PrefabUtility.SaveAsPrefabAsset(playerGo, PlayerPrefabPath);
        UnityEngine.Object.DestroyImmediate(playerGo);

        return prefab.GetComponent<NetworkObject>();
    }

    private static Material EnsurePlayerMaterial()
    {
        var material = AssetDatabase.LoadAssetAtPath<Material>(PlayerMaterialPath);
        if (material != null)
            return material;

        material = new Material(Shader.Find("Universal Render Pipeline/Lit") ?? Shader.Find("Standard"));
        material.color = new Color(0.1f, 0.75f, 0.95f);
        AssetDatabase.CreateAsset(material, PlayerMaterialPath);
        return material;
    }

    private static Material EnsureFloorMaterial()
    {
        var material = AssetDatabase.LoadAssetAtPath<Material>(FloorMaterialPath);
        if (material != null)
            return material;

        material = new Material(Shader.Find("Universal Render Pipeline/Lit") ?? Shader.Find("Standard"));
        material.color = new Color(0.36f, 0.38f, 0.4f);
        AssetDatabase.CreateAsset(material, FloorMaterialPath);
        return material;
    }

    private static DefaultPrefabObjects EnsureDefaultPrefabObjects(NetworkObject playerPrefab)
    {
        var defaultPrefabObjects = AssetDatabase.LoadAssetAtPath<DefaultPrefabObjects>(DefaultPrefabObjectsPath);
        if (defaultPrefabObjects == null)
        {
            defaultPrefabObjects = ScriptableObject.CreateInstance<DefaultPrefabObjects>();
            AssetDatabase.CreateAsset(defaultPrefabObjects, DefaultPrefabObjectsPath);
        }

        defaultPrefabObjects.Clear();
        playerPrefab.SetAssetPathHash(GetAssetPathHash(playerPrefab));
        EditorUtility.SetDirty(playerPrefab);
        defaultPrefabObjects.AddObject(playerPrefab);
        return defaultPrefabObjects;
    }

    private static ulong GetAssetPathHash(NetworkObject networkObject)
    {
        string pathAndName = $"{AssetDatabase.GetAssetPath(networkObject.gameObject)}{networkObject.gameObject.name}".Trim().ToLowerInvariant();

        HashBuilder.Clear();
        foreach (char c in pathAndName)
        {
            if ((c >= 'a' && c <= 'z') || (c >= '0' && c <= '9'))
                HashBuilder.Append(c);
        }

        return HashBuilder.ToString().GetStableHashU64();
    }

    private static Transform[] EnsureSpawnPoints()
    {
        var root = GameObject.Find("SpawnPoints") ?? new GameObject("SpawnPoints");
        Vector3[] positions =
        {
            new(-3f, 1f, 0f),
            new(3f, 1f, 0f),
            new(0f, 1f, 3f),
            new(0f, 1f, -3f),
        };

        var spawns = new Transform[positions.Length];
        for (int i = 0; i < positions.Length; i++)
        {
            string name = $"Spawn_{i + 1}";
            Transform child = root.transform.Find(name);
            if (child == null)
            {
                var go = new GameObject(name);
                child = go.transform;
                child.SetParent(root.transform);
            }

            child.position = positions[i];
            child.rotation = Quaternion.identity;
            spawns[i] = child;
            EditorUtility.SetDirty(child.gameObject);
        }

        EditorUtility.SetDirty(root);
        return spawns;
    }

    private static void EnsureFloor()
    {
        var floor = GameObject.Find("Floor");
        if (floor == null)
        {
            floor = GameObject.CreatePrimitive(PrimitiveType.Plane);
            floor.name = "Floor";
        }

        floor.transform.position = Vector3.zero;
        floor.transform.localScale = new Vector3(3f, 1f, 3f);
        floor.GetComponent<Renderer>().sharedMaterial = EnsureFloorMaterial();
        EditorUtility.SetDirty(floor);
    }

    private static void AddSceneToBuildSettings(string path)
    {
        var guid = AssetDatabase.GUIDFromAssetPath(path);
        EditorBuildSettings.scenes = new[] { new EditorBuildSettingsScene(guid, true) };
    }
}

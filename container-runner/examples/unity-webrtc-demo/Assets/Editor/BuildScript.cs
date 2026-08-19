using System;
using System.IO;
using UnityEditor;
using UnityEditor.Build;
using UnityEditor.Build.Reporting;
using UnityEngine;

/// Headless build entry points. Invoke via:
///   Unity -batchmode -quit -nographics -projectPath <p> \
///     -executeMethod BuildScript.BuildServerMac   -logFile -
///   Unity -batchmode -quit -nographics -projectPath <p> \
///     -executeMethod BuildScript.BuildServerLinux -logFile -
///
/// Produces a Unity **Dedicated Server** subtarget build (headless, no graphics).
public static class BuildScript
{
    private const string Scene = ProjectBootstrap.MainScene;

    [MenuItem("Build/Server (macOS)")]
    public static void BuildServerMac()
    {
        Build(BuildTarget.StandaloneOSX, "Builds/ServerMac/GameServer.app");
    }

    [MenuItem("Build/Server (Linux)")]
    public static void BuildServerLinux()
    {
        Build(BuildTarget.StandaloneLinux64, "Builds/ServerLinux/GameServer");
    }

    [MenuItem("Build/Client (WebGL)")]
    public static void BuildClientWebGL()
    {
        BuildPlayer(BuildTarget.WebGL, "Builds/WebGL");
    }

    /// A macOS **Player** (not Server subtarget) that runs headless as either server or
    /// client (via ServerBootstrap's `-client`). Used for the local host e2e, where we run
    /// one instance as the container-runner's child (server) and another as a client.
    [MenuItem("Build/Demo Player (macOS)")]
    public static void BuildDemoMac()
    {
        BuildPlayer(BuildTarget.StandaloneOSX, "Builds/DemoMac/GameDemo.app");
    }

    private static void BuildPlayer(BuildTarget target, string outputPath)
    {
        EditorUserBuildSettings.standaloneBuildSubtarget = StandaloneBuildSubtarget.Player;
        BuildInternal(target, outputPath, StandaloneBuildSubtarget.Player);
    }

    private static void Build(BuildTarget target, string outputPath)
    {
        EditorUserBuildSettings.standaloneBuildSubtarget = StandaloneBuildSubtarget.Server;
        BuildInternal(target, outputPath, StandaloneBuildSubtarget.Server);
    }

    private static void BuildInternal(BuildTarget target, string outputPath, StandaloneBuildSubtarget subtarget)
    {
        // Ensure the starter scene is wired to FishyWebRTC before every build.
        if (!File.Exists(Scene))
        {
            Debug.LogError($"[build] scene missing: {Scene}");
            EditorApplication.Exit(1);
        }
        ProjectBootstrap.Setup();

        var opts = new BuildPlayerOptions
        {
            scenes = new[] { Scene },
            locationPathName = outputPath,
            target = target,
            subtarget = (int)subtarget,
            options = BuildOptions.None,
        };

        Directory.CreateDirectory(Path.GetDirectoryName(outputPath));
        BuildReport report = BuildPipeline.BuildPlayer(opts);
        BuildSummary summary = report.summary;
        Debug.Log($"[build] {target} -> {outputPath} : {summary.result} ({summary.totalSize} bytes)");
        if (summary.result != BuildResult.Succeeded)
        {
            Debug.LogError($"[build] FAILED: {summary.result}");
            EditorApplication.Exit(1);
        }
    }
}

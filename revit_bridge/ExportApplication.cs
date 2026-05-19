using System.Diagnostics;
using System.Text.Json;
using Autodesk.Revit.ApplicationServices;
using Autodesk.Revit.DB;
using Autodesk.Revit.UI;
using Autodesk.Revit.UI.Events;

namespace RvtToGlb.RevitIfcExporter;

public sealed class ExportApplication : IExternalApplication
{
    private const string JobEnvVar = "RVT_TO_GLB_JOB";
    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
        WriteIndented = true
    };

    private UIControlledApplication? uiControlledApplication;
    private bool exportStarted;

    public Result OnStartup(UIControlledApplication application)
    {
        if (string.IsNullOrWhiteSpace(Environment.GetEnvironmentVariable(JobEnvVar)))
        {
            return Result.Succeeded;
        }

        uiControlledApplication = application;
        application.Idling += OnIdling;
        return Result.Succeeded;
    }

    public Result OnShutdown(UIControlledApplication application)
    {
        application.Idling -= OnIdling;
        return Result.Succeeded;
    }

    private void OnIdling(object? sender, IdlingEventArgs args)
    {
        if (exportStarted)
        {
            return;
        }

        exportStarted = true;
        if (uiControlledApplication is not null)
        {
            uiControlledApplication.Idling -= OnIdling;
        }

        var jobPath = Environment.GetEnvironmentVariable(JobEnvVar);
        RvtExportJob? job = null;
        var resultPath = FallbackResultPath(jobPath);

        try
        {
            if (string.IsNullOrWhiteSpace(jobPath))
            {
                throw new InvalidOperationException($"{JobEnvVar} is empty.");
            }

            job = JsonSerializer.Deserialize<RvtExportJob>(
                File.ReadAllText(jobPath),
                JsonOptions) ?? throw new InvalidOperationException("Job JSON is empty.");
            resultPath = job.ResultJson;

            var uiApplication = sender as UIApplication
                ?? throw new InvalidOperationException("Idling sender was not UIApplication.");
            ExportIfc(uiApplication.Application, job);
            WriteResult(resultPath, new RvtExportResult
            {
                Success = true,
                OutputIfc = job.OutputIfc,
                Message = "IFC export completed."
            });
        }
        catch (Exception ex)
        {
            WriteResult(resultPath, new RvtExportResult
            {
                Success = false,
                OutputIfc = job?.OutputIfc,
                Message = ex.ToString()
            });
        }
        finally
        {
            TryCloseRevitAfterResult();
        }
    }

    private static void ExportIfc(Application application, RvtExportJob job)
    {
        var outputDirectory = Path.GetDirectoryName(job.OutputIfc);
        var outputFileName = Path.GetFileName(job.OutputIfc);
        if (string.IsNullOrWhiteSpace(outputDirectory) || string.IsNullOrWhiteSpace(outputFileName))
        {
            throw new InvalidOperationException($"Invalid IFC output path: {job.OutputIfc}");
        }

        Directory.CreateDirectory(outputDirectory);
        if (File.Exists(job.OutputIfc))
        {
            File.Delete(job.OutputIfc);
        }

        Document? document = null;
        try
        {
            document = application.OpenDocumentFile(job.InputRvt);
            var options = BuildExportOptions(job.Options);

            using var transaction = new Transaction(document, "RVT to GLB IFC Export");
            transaction.Start();
            try
            {
                var exported = document.Export(outputDirectory, outputFileName, options);
                if (!exported)
                {
                    throw new InvalidOperationException("Document.Export returned false.");
                }

                transaction.Commit();
            }
            catch
            {
                if (transaction.GetStatus() == TransactionStatus.Started)
                {
                    transaction.RollBack();
                }

                throw;
            }
        }
        finally
        {
            if (document is not null)
            {
                try
                {
                    document.Close(false);
                }
                catch
                {
                    // Revit may already be closing; the result JSON is more important than this cleanup.
                }
            }
        }
    }

    private static IFCExportOptions BuildExportOptions(RvtExportOptions source)
    {
        var options = new IFCExportOptions
        {
            FileVersion = ParseIfcVersion(source.FileVersion),
            ExportBaseQuantities = source.ExportBaseQuantities,
            SpaceBoundaryLevel = 0
        };

        AddBool(options, "ExportIFCCommonPropertySets", source.ExportIfcCommonPropertySets);
        AddBool(options, "ExportInternalRevitPropertySets", source.ExportInternalRevitPropertySets);
        AddBool(options, "ExportBaseQuantities", source.ExportBaseQuantities);
        AddBool(options, "ExportMaterialPsets", source.ExportMaterialPsets);
        AddBool(options, "ExportUserDefinedPsets", source.ExportUserDefinedPsets);
        AddBool(options, "ExportSchedulesAsPsets", source.ExportSchedulesAsPsets);
        AddBool(options, "UseActiveViewGeometry", source.UseActiveViewGeometry);
        AddBool(options, "VisibleElementsOfCurrentView", source.VisibleElementsOfCurrentView);
        options.AddOption(
            "TessellationLevelOfDetail",
            source.TessellationLevelOfDetail.ToString(System.Globalization.CultureInfo.InvariantCulture));

        return options;
    }

    private static IFCVersion ParseIfcVersion(string? fileVersion)
    {
        return fileVersion?.Trim().ToUpperInvariant() switch
        {
            "IFC2X3" => IFCVersion.IFC2x3,
            "IFC2X3CV2" => IFCVersion.IFC2x3CV2,
            "IFC4" => IFCVersion.IFC4,
            "IFC4RV" => IFCVersion.IFC4RV,
            "IFC4DTV" => IFCVersion.IFC4DTV,
            "IFC4X3" => IFCVersion.IFC4x3,
            _ => IFCVersion.IFC2x3CV2
        };
    }

    private static void AddBool(IFCExportOptions options, string name, bool value)
    {
        options.AddOption(name, value ? "true" : "false");
    }

    private static void WriteResult(string path, RvtExportResult result)
    {
        var directory = Path.GetDirectoryName(path);
        if (!string.IsNullOrWhiteSpace(directory))
        {
            Directory.CreateDirectory(directory);
        }

        File.WriteAllText(path, JsonSerializer.Serialize(result, JsonOptions));
    }

    private static string FallbackResultPath(string? jobPath)
    {
        if (string.IsNullOrWhiteSpace(jobPath))
        {
            return Path.Combine(Path.GetTempPath(), "rvt_to_glb_export_result.json");
        }

        const string jobSuffix = ".rvt-export-job.json";
        return jobPath.EndsWith(jobSuffix, StringComparison.OrdinalIgnoreCase)
            ? jobPath[..^jobSuffix.Length] + ".rvt-export-result.json"
            : Path.ChangeExtension(jobPath, ".result.json");
    }

    private static void TryCloseRevitAfterResult()
    {
        try
        {
            Process.GetCurrentProcess().CloseMainWindow();
        }
        catch
        {
            // Rust waits for the result JSON, so graceful shutdown is best-effort.
        }
    }
}

public sealed class RvtExportJob
{
    public string InputRvt { get; set; } = "";
    public string OutputIfc { get; set; } = "";
    public string ResultJson { get; set; } = "";
    public RvtExportOptions Options { get; set; } = new();
}

public sealed class RvtExportOptions
{
    public string FileVersion { get; set; } = "IFC2x3CV2";
    public bool ExportIfcCommonPropertySets { get; set; } = true;
    public bool ExportInternalRevitPropertySets { get; set; } = true;
    public bool ExportBaseQuantities { get; set; } = true;
    public bool ExportMaterialPsets { get; set; } = true;
    public bool ExportUserDefinedPsets { get; set; }
    public bool ExportSchedulesAsPsets { get; set; }
    public bool UseActiveViewGeometry { get; set; }
    public bool VisibleElementsOfCurrentView { get; set; }
    public double TessellationLevelOfDetail { get; set; } = 0.5;
}

public sealed class RvtExportResult
{
    public bool Success { get; set; }
    public string? OutputIfc { get; set; }
    public string? Message { get; set; }
}

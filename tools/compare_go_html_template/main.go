package main

import (
	"bytes"
	"encoding/json"
	"flag"
	"fmt"
	"html/template"
	"os"
	"time"
)

type report struct {
	ParseAvgUS int64  `json:"parse_avg_us"`
	ExecAvgUS  int64  `json:"exec_avg_us"`
	Output     string `json:"output"`
	OutputLen  int    `json:"output_len"`
}

func fail(format string, args ...any) {
	_, _ = fmt.Fprintf(os.Stderr, format+"\n", args...)
	os.Exit(1)
}

func main() {
	templatePath := flag.String("template", "", "template file path")
	dataPath := flag.String("data", "", "json data file path")
	loops := flag.Int("loops", 50, "benchmark loops")
	missingKey := flag.String("missingkey", "default", "missingkey mode: default|invalid|zero|error")
	flag.Parse()

	if *templatePath == "" {
		fail("--template is required")
	}
	if *dataPath == "" {
		fail("--data is required")
	}
	if *loops <= 0 {
		fail("--loops must be greater than zero")
	}

	templateBytes, err := os.ReadFile(*templatePath)
	if err != nil {
		fail("read template: %v", err)
	}
	dataBytes, err := os.ReadFile(*dataPath)
	if err != nil {
		fail("read data: %v", err)
	}

	var data any
	if err := json.Unmarshal(dataBytes, &data); err != nil {
		fail("parse data json: %v", err)
	}

	option := "missingkey=" + *missingKey
	source := string(templateBytes)

	parseStart := time.Now()
	for i := 0; i < *loops; i++ {
		if _, err := template.New("bench").Option(option).Parse(source); err != nil {
			fail("go parse: %v", err)
		}
	}
	parseAvgUS := time.Since(parseStart).Microseconds() / int64(*loops)

	parsed, err := template.New("bench").Option(option).Parse(source)
	if err != nil {
		fail("go parse for execution: %v", err)
	}

	execStart := time.Now()
	var output string
	for i := 0; i < *loops; i++ {
		var buffer bytes.Buffer
		if err := parsed.Execute(&buffer, data); err != nil {
			fail("go execute: %v", err)
		}
		output = buffer.String()
	}
	execAvgUS := time.Since(execStart).Microseconds() / int64(*loops)

	result := report{
		ParseAvgUS: parseAvgUS,
		ExecAvgUS:  execAvgUS,
		Output:     output,
		OutputLen:  len(output),
	}

	encoder := json.NewEncoder(os.Stdout)
	encoder.SetEscapeHTML(false)
	if err := encoder.Encode(result); err != nil {
		fail("encode result: %v", err)
	}
}

import React from 'react';
import styled, { createGlobalStyle } from 'styled-components';

const GlobalStyle = createGlobalStyle`
  :root {
    --bg: linear-gradient(45deg, #0a0a0a 0%, #1a1a1a 100%);
    --card-bg: rgba(30, 30, 30, 0.95);
    --text: #e0e0e0;
    --accent: #2b7a78;
    --hover: #3daaaa;
    --section-spacing: 4rem;
  }

  * {
    margin: 0;
    padding: 0;
    box-sizing: border-box;
  }

  body {
    background: var(--bg);
    color: var(--text);
    line-height: 1.6;
    font-family: 'Inter', system-ui, sans-serif;
    min-height: 100vh;
  }

  a {
    color: var(--hover);
    text-decoration: none;
    transition: opacity 0.2s ease;

    &:hover {
      opacity: 0.8;
      text-decoration: underline;
    }
  }
`;

const Container = styled.div`
  max-width: 1400px;
  margin: 0 auto;
  padding: 4rem 2rem;
`;

const Header = styled.header`
  text-align: center;
  margin-bottom: 6rem;
  position: relative;

  h1 {
    color: var(--hover);
    font-size: 3rem;
    margin-bottom: 1.5rem;
    letter-spacing: -0.05em;
  }

  p {
    max-width: 600px;
    margin: 0 auto;
    font-size: 1.1rem;
    opacity: 0.9;
  }
`;

const ButtonGroup = styled.div`
  display: flex;
  gap: 1.5rem;
  justify-content: center;
  margin: 3rem 0;

  @media (max-width: 768px) {
    flex-direction: column;
    align-items: center;
  }
`;

const Credits = styled.footer`
  opacity: 0.8;
  font-size: 0.9rem;
  margin-top: 2rem;
  padding-top: 2rem;
  border-top: 1px solid rgba(255, 255, 255, 0.1);

  a {
    color: var(--hover);
  }
`;

const Button = styled.a`
  padding: 1rem 2rem;
  border-radius: 8px;
  font-weight: 600;
  transition: transform 0.2s ease, box-shadow 0.2s ease;
  display: inline-flex;
  align-items: center;
  gap: 0.5rem;

  &.primary {
    background: var(--accent);
    color: white;
    box-shadow: 0 4px 6px rgba(0, 0, 0, 0.1);

    &:hover {
      transform: translateY(-2px);
      box-shadow: 0 6px 12px rgba(43, 122, 120, 0.3);
    }
  }

  &.secondary {
    border: 2px solid var(--accent);
    color: var(--accent);

    &:hover {
      background: rgba(43, 122, 120, 0.1);
    }
  }
`;

const Section = styled.section`
  background: var(--card-bg);
  border-radius: 16px;
  padding: 3rem;
  margin-bottom: var(--section-spacing);
  backdrop-filter: blur(8px);
  border: 1px solid rgba(255, 255, 255, 0.1);
  box-shadow: 0 8px 32px rgba(0, 0, 0, 0.1);

  h2 {
    color: var(--hover);
    margin-bottom: 2rem;
    font-size: 2rem;
    position: relative;
    padding-bottom: 1rem;

    &::after {
      content: '';
      position: absolute;
      bottom: 0;
      left: 0;
      width: 60px;
      height: 3px;
      background: var(--accent);
      border-radius: 2px;
    }
  }
`;

const FeatureGrid = styled.div`
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
  gap: 2rem;
  margin-top: 2rem;
`;

const Feature = styled.div`
  padding: 2rem;
  background: rgba(0, 0, 0, 0.3);
  border-radius: 12px;
  border: 1px solid rgba(255, 255, 255, 0.1);

  h3 {
    margin-bottom: 1rem;
    color: var(--hover);
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  ul {
    padding-left: 1.5rem;
  }

  li {
    margin-bottom: 0.75rem;
    line-height: 1.5;
  }
`;

const ScreenshotGrid = styled.div`
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
  gap: 2rem;
  margin-top: 3rem;

  img {
    width: 100%;
    border-radius: 12px;
    border: 1px solid rgba(255, 255, 255, 0.1);
    box-shadow: 0 4px 6px rgba(0, 0, 0, 0.1);
    transition: transform 0.2s ease;

    &:hover {
      transform: translateY(-4px);
    }
  }
`;

const FullFeatureList = styled.div`
  columns: 2;
  gap: 2rem;
  margin-top: 2rem;

  @media (max-width: 768px) {
    columns: 1;
  }

  ul {
    list-style: none;
    padding: 0;
  }

  li {
    margin-bottom: 0.75rem;
    padding-left: 1.5rem;
    position: relative;

    &::before {
      content: '•';
      position: absolute;
      left: 0;
      color: var(--accent);
    }
  }
`;

const Footer = styled.footer`
  margin-top: var(--section-spacing);
  text-align: center;
  padding: 2rem;
  border-top: 1px solid rgba(255, 255, 255, 0.1);

  p {
    margin: 1rem 0;
    opacity: 0.8;
  }
`;

const App = () => {
  return (
    <>
      <head>
        <meta name="google-site-verification" content="yuUkuubLUS0OYlP0U0Oi73OaOVmL-961B3KeujN8yHM" />
      </head>
      <GlobalStyle />
      <Container>
        <Header>
          <h1>Roblox Macro Utilities</h1>
          <p>Open-source Windows automation tool with customizable interface and portable execution</p>
          
          <ButtonGroup>
            <Button 
              href="https://github.com/Spencer0187/Roblox-Macro-Utilities/releases/latest" 
              className="primary"
            >
              Download Portable EXE
            </Button>
            <Button 
              href="https://github.com/Spencer0187/Roblox-Macro-Utilities" 
              className="secondary"
            >
              View Source Code
            </Button>
          </ButtonGroup>
        </Header>

        <Section>
          <h2>Key Features</h2>
          <FeatureGrid>
            <Feature>
              <h3>🖱️ Custom Interface</h3>
              <ul>
                <li>Drag-and-drop button rearrangement</li>
                <li>Resizable window to fit your workflow</li>
                <li>Automatic layout saving</li>
                <li>Clean, modern dark theme</li>
              </ul>
            </Feature>
            
            <Feature>
              <h3>⚡ Performance</h3>
              <ul>
                <li>Single portable executable</li>
                <li>No installation required</li>
                <li>Lightweight (under 5MB)</li>
                <li>Windows 10/11 optimized</li>
              </ul>
            </Feature>

            <Feature>
              <h3>🛡️ Safety</h3>
              <ul>
                <li>No memory modification</li>
                <li>No background services</li>
                <li>Local config storage</li>
                <li>Open source verification</li>
              </ul>
            </Feature>
          </FeatureGrid>
        </Section>

        <Section>
          <h2>Complete Feature List</h2>
          <FullFeatureList>
            <ul>
              {[
                "Anti-AFK (works in background)",
                "Customizable UI buttons (drag to rearrange)",
                "Helicopter High Jump",
                "Speedglitch toggle",
                "Automatic Ledge Bouncing",
                "Laugh Clipping automation",
                "Dropless Item Desync",
                "Freeze Macro",
                "Unequip Speedglitch",
                "Wallhop Macro",
                "Lag High Jump (14 Studs)",
                "One-Frame Keypress",
                "Wall-Walk assistance",
                "Item-Clip automation",
                "Key/Button Spammer",
                "Resizable interface window",
                "Portable configuration",
                "Automatic updates"
              ].map((feature, index) => (
                <li key={index}>{feature}</li>
              ))}
            </ul>
          </FullFeatureList>
        </Section>

        <Section>
          <h2>Interface Preview</h2>
          <ScreenshotGrid>
            <img src="path-to-screenshot-1.jpg" alt="Main interface showing draggable buttons" />
            <img src="path-to-screenshot-2.jpg" alt="Settings customization preview" />
            <img src="path-to-screenshot-3.jpg" alt="Resizable window demonstration" />
          </ScreenshotGrid>
        </Section>

        <Section>
          <h2>Troubleshooting</h2>
          <FeatureGrid>
            <Feature>
              <h3>Common Issues</h3>
              <ul>
                <li><strong>DLL errors:</strong> Install <a href="https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist">Visual C++ Redist</a></li>
                <li><strong>Keybind issues:</strong> Restart PC</li>
                <li><strong>Launch failure:</strong> Unblock in file properties</li>
              </ul>
            </Feature>
            
            <Feature>
              <h3>Technical Specs</h3>
              <ul>
                <li>C++17 with ImGui</li>
                <li>Windows API hooks</li>
                <li>Portable configuration</li>
                <li>MIT Licensed</li>
              </ul>
            </Feature>
          </FeatureGrid>
        </Section>

        <Footer>
          <ButtonGroup>
            <Button 
              href="https://github.com/Spencer0187/Roblox-Macro-Utilities/releases/latest" 
              className="primary"
            >
              Download Now
            </Button>
          </ButtonGroup>
          <p>Need help? Join our <a href="https://discord.gg/roblox-glitching-community-998572881892094012">Discord support community</a></p>
          <Credits>
            <p>Built with:</p>
            <ul>
              <li><a href="https://github.com/ocornut/imgui">ImGui</a></li>
              <li><a href="https://github.com/craftwar/suspend">suspend</a></li>
            </ul>
          </Credits>
        </Footer>
      </Container>
    </>
  );
};

export default App;
